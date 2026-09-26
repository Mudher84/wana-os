//! One xdg_toplevel window with a memfd-backed XRGB8888 buffer, and the
//! steps of mapping it (configure handshake, then a buffer with a frame
//! callback).

use crate::client::{Connection, Proxy, Req, Val};
use crate::input::Devices;
use crate::wait_for;
use std::fs::File;
use std::io::Write;
use std::os::unix::io::{AsRawFd, FromRawFd};
use wana_log::info;
use wana_wayland::protocols::{wayland, xdg_shell};

const FORMAT_XRGB8888: u32 = 1;
pub const BORDER_PX: i32 = 8;

extern "C" {
    fn memfd_create(name: *const std::os::raw::c_char, flags: u32) -> i32;
}

/// The globals a window needs.
#[derive(Debug, Clone, Copy)]
pub struct Shell {
    pub compositor: Proxy,
    pub shm: Proxy,
    pub wm_base: Proxy,
}

#[derive(Debug)]
pub struct Window {
    /// Name used in the client's log lines.
    pub name: &'static str,
    pub surface: Proxy,
    pub xdg: Proxy,
    pub toplevel: Proxy,
    pub buffer: Proxy,
    /// The pool's file (kept open; `--truncate-pool` shrinks it).
    pub file: File,
    pub width: i32,
    pub height: i32,
}

/// XRGB8888 little-endian pixels: `fill` inside a `border` frame.
pub fn pattern(width: i32, height: i32, fill: u32, border: u32) -> Vec<u8> {
    let mut px = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let edge =
                x < BORDER_PX || y < BORDER_PX || x >= width - BORDER_PX || y >= height - BORDER_PX;
            let rgb = if edge { border } else { fill };
            px.extend_from_slice(&(0xFF00_0000 | rgb).to_le_bytes());
        }
    }
    px
}

impl Window {
    /// Creates the pool, buffer, surface and toplevel (not yet committed).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        conn: &Connection,
        shell: &Shell,
        name: &'static str,
        title: &str,
        width: i32,
        height: i32,
        fill: u32,
        border: u32,
    ) -> Result<Window, String> {
        let bytes = pattern(width, height, fill, border);
        // SAFETY: NUL-terminated name; the fd is owned by the File below.
        let fd = unsafe { memfd_create(c"wana-wl-test".as_ptr(), 0) };
        if fd < 0 {
            return Err(format!("memfd_create: {}", std::io::Error::last_os_error()));
        }
        // SAFETY: fresh fd from memfd_create.
        let mut file = unsafe { File::from_raw_fd(fd) };
        file.write_all(&bytes)
            .map_err(|e| format!("pool write: {e}"))?;
        let pool = conn
            .request(
                shell.shm,
                wayland::wl_shm::request::CREATE_POOL,
                Some((&wayland::WL_SHM_POOL_INTERFACE, 1)),
                &[
                    Req::NewId,
                    Req::Fd(file.as_raw_fd()),
                    Req::Int(bytes.len() as i32),
                ],
            )?
            .expect("pool");
        let buffer = conn
            .request(
                pool,
                wayland::wl_shm_pool::request::CREATE_BUFFER,
                Some((&wayland::WL_BUFFER_INTERFACE, 1)),
                &[
                    Req::NewId,
                    Req::Int(0),
                    Req::Int(width),
                    Req::Int(height),
                    Req::Int(width * 4),
                    Req::Uint(FORMAT_XRGB8888),
                ],
            )?
            .expect("buffer");
        let surface = conn
            .request(
                shell.compositor,
                wayland::wl_compositor::request::CREATE_SURFACE,
                Some((&wayland::WL_SURFACE_INTERFACE, 4)),
                &[Req::NewId],
            )?
            .expect("surface");
        let xdg = conn
            .request(
                shell.wm_base,
                xdg_shell::xdg_wm_base::request::GET_XDG_SURFACE,
                Some((&xdg_shell::XDG_SURFACE_INTERFACE, 1)),
                &[Req::NewId, Req::Object(Some(surface))],
            )?
            .expect("xdg_surface");
        let toplevel = conn
            .request(
                xdg,
                xdg_shell::xdg_surface::request::GET_TOPLEVEL,
                Some((&xdg_shell::XDG_TOPLEVEL_INTERFACE, 1)),
                &[Req::NewId],
            )?
            .expect("xdg_toplevel");
        conn.request(
            toplevel,
            xdg_shell::xdg_toplevel::request::SET_TITLE,
            None,
            &[Req::Str(title)],
        )?;
        conn.request(
            toplevel,
            xdg_shell::xdg_toplevel::request::SET_APP_ID,
            None,
            &[Req::Str("org.wana.test")],
        )?;
        Ok(Window {
            name,
            surface,
            xdg,
            toplevel,
            buffer,
            file,
            width,
            height,
        })
    }

    /// Attaches the buffer (damaging all of it) and commits, optionally
    /// with a frame callback.
    pub fn attach_commit(
        &self,
        conn: &Connection,
        with_frame: bool,
    ) -> Result<Option<Proxy>, String> {
        let s = self.surface;
        conn.request(
            s,
            wayland::wl_surface::request::ATTACH,
            None,
            &[Req::Object(Some(self.buffer)), Req::Int(0), Req::Int(0)],
        )?;
        conn.request(
            s,
            wayland::wl_surface::request::DAMAGE,
            None,
            &[
                Req::Int(0),
                Req::Int(0),
                Req::Int(self.width),
                Req::Int(self.height),
            ],
        )?;
        let cb = if with_frame {
            conn.request(
                s,
                wayland::wl_surface::request::FRAME,
                Some((&wayland::WL_CALLBACK_INTERFACE, 1)),
                &[Req::NewId],
            )?
        } else {
            None
        };
        conn.request(s, wayland::wl_surface::request::COMMIT, None, &[])?;
        Ok(cb)
    }

    /// Initial commit without a buffer, then waits for the configure and
    /// acks it.
    pub fn configure(
        &self,
        conn: &Connection,
        wm_base: Proxy,
        devices: Option<&mut Devices>,
    ) -> Result<(), String> {
        conn.request(
            self.surface,
            wayland::wl_surface::request::COMMIT,
            None,
            &[],
        )?;
        let xdg = self.xdg;
        let serial = wait_for(conn, wm_base, devices, |ev| {
            (ev.target == xdg && ev.opcode == xdg_shell::xdg_surface::event::CONFIGURE)
                .then(|| match ev.args.first() {
                    Some(Val::Uint(s)) => Some(*s),
                    _ => None,
                })
                .flatten()
        })?;
        info!(
            crate::LOG,
            "client: configure received (serial {serial}); acking"
        );
        conn.request(
            xdg,
            xdg_shell::xdg_surface::request::ACK_CONFIGURE,
            None,
            &[Req::Uint(serial)],
        )?;
        Ok(())
    }

    /// Commits the buffer with a frame callback and waits until the
    /// compositor reports the frame presented.
    pub fn present(
        &self,
        conn: &Connection,
        wm_base: Proxy,
        devices: Option<&mut Devices>,
    ) -> Result<(), String> {
        let frame = self.attach_commit(conn, true)?.expect("frame callback");
        info!(
            crate::LOG,
            "client: committed {}x{} XRGB8888 buffer with a frame callback",
            self.width,
            self.height
        );
        let buffer = self.buffer;
        let mut released = false;
        let time = wait_for(conn, wm_base, devices, |ev| {
            if ev.target == buffer && ev.opcode == wayland::wl_buffer::event::RELEASE {
                released = true;
            }
            (ev.target == frame && ev.opcode == wayland::wl_callback::event::DONE)
                .then(|| match ev.args.first() {
                    Some(Val::Uint(t)) => Some(*t),
                    _ => None,
                })
                .flatten()
        })?;
        info!(
            crate::LOG,
            "client: frame presented (callback done at {time} ms); buffer {}",
            if released {
                "released"
            } else {
                "not released yet"
            }
        );
        Ok(())
    }

    pub fn destroy(&self, conn: &Connection) {
        conn.destroy(self.toplevel, xdg_shell::xdg_toplevel::request::DESTROY);
        conn.destroy(self.xdg, xdg_shell::xdg_surface::request::DESTROY);
        conn.destroy(self.surface, wayland::wl_surface::request::DESTROY);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_has_border_and_fill() {
        let (w, h) = (480, 320);
        let px = pattern(w, h, 0x4F8CFF, 0xFFFFFF);
        assert_eq!(px.len(), (w * h * 4) as usize);
        let at = |x: i32, y: i32| {
            let i = ((y * w + x) * 4) as usize;
            u32::from_le_bytes(px[i..i + 4].try_into().unwrap()) & 0xFF_FFFF
        };
        assert_eq!(at(0, 0), 0xFFFFFF);
        assert_eq!(at(w - 1, h - 1), 0xFFFFFF);
        assert_eq!(at(BORDER_PX, BORDER_PX), 0x4F8CFF);
        assert_eq!(at(w / 2, h / 2), 0x4F8CFF);
        // Little-endian XRGB: bytes B, G, R, X.
        let c = ((h / 2 * w + w / 2) * 4) as usize;
        assert_eq!(&px[c..c + 3], &[0xFF, 0x8C, 0x4F]);
    }
}
