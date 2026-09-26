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
//! `--input TEXT` (Phase 10 step 4) also binds wl_seat, waits for a pointer
//! and a keyboard, compiles the keymap the compositor sends, and after the
//! window is on screen reports `client: ready for input`; it then waits for
//! the pointer to enter the window, a left click, and TEXT typed (decoded
//! with that keymap and the compositor's modifier state), and exits 0.
//!
//! Lines are tagged `[COMPOSITOR] info: client: ...`.

mod client;

use client::{Connection, Event, Proxy, Req, Val};
use std::fs::File;
use std::io::Write;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process::ExitCode;
use std::time::Duration;
use wana_input::keyboard::{Keyboard, Modifiers};
use wana_log::{debug, error, info, Subsystem};
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
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
}
const PROT_READ: i32 = 1;
const MAP_PRIVATE: i32 = 2;
const MAP_FAILED: *mut u8 = !0usize as *mut u8;
const BTN_LEFT: u32 = 0x110;
const CAP_POINTER: u32 = 1;
const CAP_KEYBOARD: u32 = 2;

fn main() -> ExitCode {
    wana_log::init_from_env();
    let mut mode = Mode::Window;
    let mut hold = 0u64;
    let mut input = None;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--attach-before-configure" => mode = Mode::AttachBeforeConfigure,
            "--truncate-pool" => mode = Mode::TruncatePool,
            "--hold" => hold = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--input" => input = it.next(),
            other => {
                error!(LOG, "client: unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    match run(mode, hold, input.as_deref()) {
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
    seat: Option<(u32, u32)>,
}

fn run(mode: Mode, hold: u64, input: Option<&str>) -> Result<(), String> {
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
        seat: None,
    };
    while let Some(ev) = conn.next_event() {
        if ev.target == registry && ev.opcode == wayland::wl_registry::event::GLOBAL {
            if let [Val::Uint(name), Val::Str(iface), Val::Uint(version)] = &ev.args[..] {
                match iface.as_str() {
                    "wl_compositor" => g.compositor = Some((*name, *version)),
                    "wl_shm" => g.shm = Some((*name, *version)),
                    "xdg_wm_base" => g.wm_base = Some((*name, *version)),
                    "wl_seat" => g.seat = Some((*name, *version)),
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
    let mut devices = match input {
        Some(_) => {
            let seat = bind(g.seat, &wayland::WL_SEAT_INTERFACE, 7, "wl_seat")?;
            Some(Devices::new(&conn, seat, surface)?)
        }
        None => None,
    };
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
    let serial = wait_for(&conn, wm_base, devices.as_mut(), |ev| {
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
    let time = wait_for(&conn, wm_base, devices.as_mut(), |ev| {
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
    if let (Some(dev), Some(text)) = (devices.as_mut(), input) {
        dev.wait_for_input(&conn, wm_base, text)?;
    }
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
/// Every event also goes to `devices` (input can arrive at any time, e.g.
/// the keymap and keyboard enter while waiting for a configure). Events
/// already queued are handled before blocking for more.
fn wait_for<T>(
    conn: &Connection,
    wm_base: Proxy,
    mut devices: Option<&mut Devices>,
    mut pick: impl FnMut(&client::Event) -> Option<T>,
) -> Result<T, String> {
    for _ in 0..1000 {
        while let Some(ev) = conn.next_event() {
            if let Some(d) = devices.as_deref_mut() {
                d.event(&ev)?;
            }
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
        conn.dispatch()?;
    }
    Err("expected event never arrived".into())
}

/// The seat's pointer and keyboard, and what arrived through them.
struct Devices {
    /// The window's wl_surface (focus events name it).
    surface: Proxy,
    pointer: Proxy,
    keyboard: Proxy,
    xkb: Option<Keyboard>,
    /// The window has keyboard focus.
    focused: bool,
    entered: bool,
    motions: u32,
    left_down: bool,
    clicks: u32,
    /// Last pointer position on the window (surface-local).
    at: (f64, f64),
    typed: String,
}

impl Devices {
    /// Waits for pointer + keyboard capabilities, then creates both objects
    /// (before the window maps, so the keyboard enter on map reaches them).
    fn new(conn: &Connection, seat: Proxy, surface: Proxy) -> Result<Devices, String> {
        let mut caps = 0;
        for _ in 0..1000 {
            conn.roundtrip()?;
            while let Some(ev) = conn.next_event() {
                if ev.target == seat && ev.opcode == wayland::wl_seat::event::CAPABILITIES {
                    if let Some(Val::Uint(c)) = ev.args.first() {
                        caps = *c;
                    }
                }
            }
            if caps & (CAP_POINTER | CAP_KEYBOARD) == CAP_POINTER | CAP_KEYBOARD {
                break;
            }
            conn.dispatch()?;
        }
        info!(
            LOG,
            "client: seat capabilities {caps:#x} (pointer + keyboard)"
        );
        let pointer = conn
            .request(
                seat,
                wayland::wl_seat::request::GET_POINTER,
                Some((&wayland::WL_POINTER_INTERFACE, 7)),
                &[Req::NewId],
            )?
            .expect("pointer");
        let keyboard = conn
            .request(
                seat,
                wayland::wl_seat::request::GET_KEYBOARD,
                Some((&wayland::WL_KEYBOARD_INTERFACE, 7)),
                &[Req::NewId],
            )?
            .expect("keyboard");
        Ok(Devices {
            surface,
            pointer,
            keyboard,
            xkb: None,
            focused: false,
            entered: false,
            motions: 0,
            left_down: false,
            clicks: 0,
            at: (0.0, 0.0),
            typed: String::new(),
        })
    }

    fn done(&self, text: &str) -> bool {
        self.entered && self.clicks > 0 && self.typed.ends_with(text)
    }

    /// Dispatches until the window has keyboard focus, reports readiness,
    /// then until the pointer entered, a left click and `text` arrived.
    fn wait_for_input(
        &mut self,
        conn: &Connection,
        wm_base: Proxy,
        text: &str,
    ) -> Result<(), String> {
        if !(self.focused && self.xkb.is_some()) {
            wait_for(conn, wm_base, None, |ev| {
                if let Err(e) = self.event(ev) {
                    return Some(Err(e));
                }
                (self.focused && self.xkb.is_some()).then_some(Ok(()))
            })??;
        }
        info!(
            LOG,
            "client: ready for input (keyboard focus, keymap compiled)"
        );
        if !self.done(text) {
            wait_for(conn, wm_base, None, |ev| {
                if let Err(e) = self.event(ev) {
                    return Some(Err(e));
                }
                self.done(text).then_some(Ok(()))
            })??;
        }
        info!(
            LOG,
            "client: input received: typed {:?}, pointer entered, {} motion event(s), {} left click(s)",
            self.typed,
            self.motions,
            self.clicks
        );
        Ok(())
    }

    fn event(&mut self, ev: &Event) -> Result<(), String> {
        let surface = self.surface;
        use wayland::{wl_keyboard::event as kev, wl_pointer::event as pev};
        let fixed = |v: &Val| match v {
            Val::Int(f) => f64::from(*f) / 256.0,
            _ => 0.0,
        };
        let on_window = |v: Option<&Val>| matches!(v, Some(Val::Object(Some(p))) if *p == surface);
        if ev.target == self.keyboard {
            match (ev.opcode, &ev.args[..]) {
                (kev::KEYMAP, [Val::Uint(format), Val::Int(fd), Val::Uint(size)]) => {
                    self.xkb = Some(load_keymap(*format, *fd, *size)?);
                }
                (kev::ENTER, [_, s, Val::Array(keys)]) if on_window(Some(s)) => {
                    self.focused = true;
                    info!(
                        LOG,
                        "client: keyboard focus on the window ({} key(s) held)",
                        keys.len() / 4
                    );
                }
                (kev::LEAVE, [_, s]) if on_window(Some(s)) => {
                    self.focused = false;
                    info!(LOG, "client: keyboard focus left the window");
                }
                (kev::KEY, [_, _, Val::Uint(key), Val::Uint(state)]) => {
                    let xkb = self.xkb.as_ref().ok_or("key before keymap")?;
                    if !self.focused {
                        return Err(format!("key {key} without keyboard focus"));
                    }
                    if *state == 1 {
                        let k = xkb.lookup(*key);
                        info!(
                            LOG,
                            "client: key {key} pressed: {} text {:?}", k.keysym, k.text
                        );
                        self.typed.push_str(&k.text);
                    }
                }
                (
                    kev::MODIFIERS,
                    [_, Val::Uint(depressed), Val::Uint(latched), Val::Uint(locked), Val::Uint(group)],
                ) => {
                    let m = Modifiers {
                        depressed: *depressed,
                        latched: *latched,
                        locked: *locked,
                        group: *group,
                    };
                    debug!(LOG, "client: modifiers {m:?}");
                    if let Some(xkb) = self.xkb.as_mut() {
                        xkb.set_modifiers(&m);
                    }
                }
                _ => {}
            }
        } else if ev.target == self.pointer {
            match (ev.opcode, &ev.args[..]) {
                (pev::ENTER, [_, s, x, y]) if on_window(Some(s)) => {
                    self.entered = true;
                    self.at = (fixed(x), fixed(y));
                    info!(
                        LOG,
                        "client: pointer entered the window at {:.1},{:.1}", self.at.0, self.at.1
                    );
                }
                (pev::LEAVE, [_, s]) if on_window(Some(s)) => {
                    self.entered = false;
                    info!(LOG, "client: pointer left the window");
                }
                (pev::MOTION, [_, x, y]) => {
                    self.motions += 1;
                    self.at = (fixed(x), fixed(y));
                }
                (pev::BUTTON, [_, _, Val::Uint(button), Val::Uint(state)]) => {
                    if !self.entered {
                        return Err(format!("button {button} without pointer focus"));
                    }
                    if *button == BTN_LEFT {
                        if *state == 1 {
                            self.left_down = true;
                        } else if self.left_down {
                            self.left_down = false;
                            self.clicks += 1;
                            info!(
                                LOG,
                                "client: left click at {:.1},{:.1}", self.at.0, self.at.1
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

/// Maps the keymap file the compositor sent (MAP_PRIVATE, as wl_keyboard
/// v7 requires) and compiles it.
fn load_keymap(format: u32, fd: i32, size: u32) -> Result<Keyboard, String> {
    // SAFETY: the fd was received with the event and is owned from here.
    let file = unsafe { File::from_raw_fd(fd) };
    if format != 1 {
        return Err(format!("keymap format {format}, expected 1 (xkb v1)"));
    }
    let len = size as usize;
    // SAFETY: read-only private mapping of `len` bytes of the file.
    let p = unsafe {
        mmap(
            std::ptr::null_mut(),
            len,
            PROT_READ,
            MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        )
    };
    if p == MAP_FAILED {
        return Err(format!("keymap mmap: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: the mapping is `len` bytes; unmapped right after copying.
    let bytes = unsafe { std::slice::from_raw_parts(p, len) }.to_vec();
    // SAFETY: mapping from above.
    unsafe { munmap(p, len) };
    let text = std::ffi::CStr::from_bytes_until_nul(&bytes)
        .map_err(|_| "keymap is not NUL-terminated".to_string())?
        .to_str()
        .map_err(|e| format!("keymap: {e}"))?;
    let kb = Keyboard::from_string(text)?;
    info!(
        LOG,
        "client: keymap received: {} bytes, layout {}",
        size,
        kb.layout_name()
    );
    Ok(kb)
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
