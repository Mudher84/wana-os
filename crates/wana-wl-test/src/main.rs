//! wana-wl-test: a Wayland client that proves a window reaches the screen.
//!
//! Default: binds wl_compositor, wl_shm and xdg_wm_base, creates an
//! xdg_toplevel, does the configure handshake, draws a 480x320 XRGB8888
//! buffer (accent color, 8 px white border: the Wana test palette) into a
//! memfd pool, commits it with a frame callback, and waits until the
//! compositor reports the frame presented. Then it holds the window
//! (`--hold SECONDS`) so a screenshot can be taken, and exits 0.
//!
//! Protocol failure modes, each expected to end in a specific error:
//! - `--attach-before-configure`: commits a buffer before acking the
//!   configure; expects xdg_surface error 3 (unconfigured_buffer);
//! - `--truncate-pool`: shrinks the pool's file to 0 after creating the
//!   buffer; expects wl_buffer error 2 (wl_shm invalid_fd) and a compositor
//!   that keeps running.
//!
//! Lines are tagged `[COMPOSITOR] info: client: ...`.

mod client;

use client::{Connection, Proxy, Req, Val};
use std::fs::File;
use std::io::Write;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process::ExitCode;
use std::time::Duration;
use wana_log::{error, info, Subsystem};
use wana_render::scene::{ACCENT, BORDER};
use wana_wayland::protocols::{wayland, xdg_shell};

const LOG: Subsystem = Subsystem::Compositor;
const WIDTH: i32 = 480;
const HEIGHT: i32 = 320;
const BORDER_PX: i32 = 8;
const FORMAT_XRGB8888: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Window,
    AttachBeforeConfigure,
    TruncatePool,
}

extern "C" {
    fn memfd_create(name: *const std::os::raw::c_char, flags: u32) -> i32;
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    let mut mode = Mode::Window;
    let mut hold = 0u64;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--attach-before-configure" => mode = Mode::AttachBeforeConfigure,
            "--truncate-pool" => mode = Mode::TruncatePool,
            "--hold" => hold = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            other => {
                error!(LOG, "client: unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    match run(mode, hold) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "client: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The Wana test pattern as XRGB8888 little-endian pixels.
fn pattern() -> Vec<u8> {
    let mut px = Vec::with_capacity((WIDTH * HEIGHT * 4) as usize);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let edge =
                x < BORDER_PX || y < BORDER_PX || x >= WIDTH - BORDER_PX || y >= HEIGHT - BORDER_PX;
            let rgb = if edge { BORDER } else { ACCENT };
            px.extend_from_slice(&(0xFF00_0000 | rgb).to_le_bytes());
        }
    }
    px
}

struct Globals {
    compositor: Option<(u32, u32)>,
    shm: Option<(u32, u32)>,
    wm_base: Option<(u32, u32)>,
}

fn run(mode: Mode, hold: u64) -> Result<(), String> {
    let conn = Connection::connect()?;
    info!(LOG, "client: connected");
    let registry = conn
        .request(
            conn.display(),
            wayland::wl_display::request::GET_REGISTRY,
            Some((&wayland::WL_REGISTRY_INTERFACE, 1)),
            &[Req::NewId],
        )?
        .expect("registry");
    conn.roundtrip()?;
    let mut g = Globals {
        compositor: None,
        shm: None,
        wm_base: None,
    };
    while let Some(ev) = conn.next_event() {
        if ev.target == registry && ev.opcode == wayland::wl_registry::event::GLOBAL {
            if let [Val::Uint(name), Val::Str(iface), Val::Uint(version)] = &ev.args[..] {
                match iface.as_str() {
                    "wl_compositor" => g.compositor = Some((*name, *version)),
                    "wl_shm" => g.shm = Some((*name, *version)),
                    "xdg_wm_base" => g.wm_base = Some((*name, *version)),
                    _ => {}
                }
            }
        }
    }
    let bind = |glob: Option<(u32, u32)>,
                iface: &'static wana_wayland::sys::wl_interface,
                want: u32,
                name: &str|
     -> Result<Proxy, String> {
        let (id, ver) = glob.ok_or(format!("compositor has no {name}"))?;
        let v = ver.min(want);
        conn.request(
            registry,
            wayland::wl_registry::request::BIND,
            Some((iface, v)),
            &[Req::Uint(id), Req::Str(name), Req::Uint(v), Req::NewId],
        )
        .map(|p| p.expect("bound object"))
    };
    let compositor = bind(
        g.compositor,
        &wayland::WL_COMPOSITOR_INTERFACE,
        4,
        "wl_compositor",
    )?;
    let shm = bind(g.shm, &wayland::WL_SHM_INTERFACE, 1, "wl_shm")?;
    let wm_base = bind(
        g.wm_base,
        &xdg_shell::XDG_WM_BASE_INTERFACE,
        1,
        "xdg_wm_base",
    )?;
    info!(LOG, "client: bound wl_compositor, wl_shm, xdg_wm_base");

    // Pool and buffer: a memfd with the test pattern.
    let bytes = pattern();
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
            shm,
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
                Req::Int(WIDTH),
                Req::Int(HEIGHT),
                Req::Int(WIDTH * 4),
                Req::Uint(FORMAT_XRGB8888),
            ],
        )?
        .expect("buffer");

    // Surface with the xdg_toplevel role.
    let surface = conn
        .request(
            compositor,
            wayland::wl_compositor::request::CREATE_SURFACE,
            Some((&wayland::WL_SURFACE_INTERFACE, 4)),
            &[Req::NewId],
        )?
        .expect("surface");
    let xdg = conn
        .request(
            wm_base,
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
        &[Req::Str("wana-wl-test")],
    )?;
    conn.request(
        toplevel,
        xdg_shell::xdg_toplevel::request::SET_APP_ID,
        None,
        &[Req::Str("org.wana.test")],
    )?;

    let attach_commit = |with_frame: bool| -> Result<Option<Proxy>, String> {
        conn.request(
            surface,
            wayland::wl_surface::request::ATTACH,
            None,
            &[Req::Object(Some(buffer)), Req::Int(0), Req::Int(0)],
        )?;
        conn.request(
            surface,
            wayland::wl_surface::request::DAMAGE,
            None,
            &[Req::Int(0), Req::Int(0), Req::Int(WIDTH), Req::Int(HEIGHT)],
        )?;
        let cb = if with_frame {
            conn.request(
                surface,
                wayland::wl_surface::request::FRAME,
                Some((&wayland::WL_CALLBACK_INTERFACE, 1)),
                &[Req::NewId],
            )?
        } else {
            None
        };
        conn.request(surface, wayland::wl_surface::request::COMMIT, None, &[])?;
        Ok(cb)
    };

    if mode == Mode::AttachBeforeConfigure {
        info!(
            LOG,
            "client: attaching a buffer before the first configure (protocol violation on purpose)"
        );
        attach_commit(false)?;
        return expect_error(&conn, "xdg_surface", 3);
    }

    // Initial commit without a buffer: the compositor must configure.
    conn.request(surface, wayland::wl_surface::request::COMMIT, None, &[])?;
    let serial = wait_for(&conn, wm_base, |ev| {
        (ev.target == xdg && ev.opcode == xdg_shell::xdg_surface::event::CONFIGURE)
            .then(|| match ev.args.first() {
                Some(Val::Uint(s)) => Some(*s),
                _ => None,
            })
            .flatten()
    })?;
    info!(LOG, "client: configure received (serial {serial}); acking");
    conn.request(
        xdg,
        xdg_shell::xdg_surface::request::ACK_CONFIGURE,
        None,
        &[Req::Uint(serial)],
    )?;

    if mode == Mode::TruncatePool {
        file.set_len(0).map_err(|e| format!("ftruncate: {e}"))?;
        info!(LOG, "client: pool file truncated to 0 bytes; committing the buffer (protocol violation on purpose)");
        attach_commit(false)?;
        return expect_error(&conn, "wl_buffer", 2);
    }

    let frame = attach_commit(true)?.expect("frame callback");
    info!(
        LOG,
        "client: committed {WIDTH}x{HEIGHT} XRGB8888 buffer with a frame callback"
    );
    let mut released = false;
    let time = wait_for(&conn, wm_base, |ev| {
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
        LOG,
        "client: frame presented (callback done at {time} ms); buffer {}",
        if released {
            "released"
        } else {
            "not released yet"
        }
    );
    if hold > 0 {
        info!(LOG, "client: holding window for {hold}s");
        std::thread::sleep(Duration::from_secs(hold));
    }
    conn.destroy(toplevel, xdg_shell::xdg_toplevel::request::DESTROY);
    conn.destroy(xdg, xdg_shell::xdg_surface::request::DESTROY);
    conn.destroy(surface, wayland::wl_surface::request::DESTROY);
    conn.roundtrip()?;
    info!(LOG, "client: done");
    Ok(())
}

/// Dispatches events until `pick` returns a value; answers pings.
fn wait_for<T>(
    conn: &Connection,
    wm_base: Proxy,
    mut pick: impl FnMut(&client::Event) -> Option<T>,
) -> Result<T, String> {
    for _ in 0..1000 {
        conn.dispatch()?;
        while let Some(ev) = conn.next_event() {
            if ev.target == wm_base && ev.opcode == xdg_shell::xdg_wm_base::event::PING {
                if let Some(Val::Uint(s)) = ev.args.first() {
                    conn.request(
                        wm_base,
                        xdg_shell::xdg_wm_base::request::PONG,
                        None,
                        &[Req::Uint(*s)],
                    )?;
                }
            }
            if let Some(v) = pick(&ev) {
                return Ok(v);
            }
        }
    }
    Err("expected event never arrived".into())
}

/// Expects the connection to end with `interface` error `code`.
fn expect_error(conn: &Connection, interface: &str, code: u32) -> Result<(), String> {
    let err = match conn.roundtrip() {
        Ok(()) => conn.roundtrip().err(),
        Err(e) => Some(e),
    };
    match conn.protocol_error() {
        Some((iface, id, c)) if iface == interface && c == code => {
            info!(
                LOG,
                "client: got the expected protocol error: {iface}@{id} code {c}"
            );
            Ok(())
        }
        Some((iface, id, c)) => Err(format!(
            "wrong protocol error: {iface}@{id} code {c} (expected {interface} code {code})"
        )),
        None => Err(format!(
            "no protocol error (expected {interface} code {code}); {err:?}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_has_border_and_accent() {
        let px = pattern();
        assert_eq!(px.len(), (WIDTH * HEIGHT * 4) as usize);
        let at = |x: i32, y: i32| {
            let i = ((y * WIDTH + x) * 4) as usize;
            u32::from_le_bytes(px[i..i + 4].try_into().unwrap()) & 0xFF_FFFF
        };
        assert_eq!(at(0, 0), BORDER);
        assert_eq!(at(WIDTH - 1, HEIGHT - 1), BORDER);
        assert_eq!(at(BORDER_PX, BORDER_PX), ACCENT);
        assert_eq!(at(WIDTH / 2, HEIGHT / 2), ACCENT);
        // Little-endian XRGB: bytes B, G, R, X.
        let c = ((HEIGHT / 2 * WIDTH + WIDTH / 2) * 4) as usize;
        assert_eq!(&px[c..c + 3], &[0xFF, 0x8C, 0x4F]);
    }
}
