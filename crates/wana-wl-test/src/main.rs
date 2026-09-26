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
//! Input (Phase 10 step 4; both bind wl_seat, wait for a pointer and a
//! keyboard, compile the keymap the compositor sends, and report
//! `client: ready for input` once the expected window has keyboard focus):
//! - `--input TEXT`: one window ("main"); waits for the pointer on it, a left
//!   click on it, and TEXT typed into it;
//! - `--zorder TEXT`: window "back" (larger than the screen, so placed at
//!   0,0, in amber), then window "front" (480x320, accent) on top of it,
//!   which gets keyboard focus. Waits for a click on "back" (visible only
//!   outside "front", e.g. at the top-left corner), keyboard focus moving
//!   to "back", and TEXT typed into it. The compositor must also raise
//!   "back" above "front", which a screenshot shows. On every pointer
//!   enter the client sets a 64x64 magenta cursor surface (hotspot 0,0).
//!
//! `--text`: a window showing Arabic and Latin text drawn by wana-text (the
//! whole text stack: pinned fonts, shaping, BiDi, layout, rasterizer). Logs
//! the rendering's SHA-256 (it is deterministic) and a fully inked pixel.
//! Fonts from `--fonts DIR` (default /usr/share/fonts/wana).
//!
//! `--expect-global NAME` / `--expect-no-global NAME` (repeatable): only
//! checks which globals this client sees, then exits (0 if all as expected).
//! Run as the shell (on its private connection) and as an ordinary client,
//! it tests the privilege filter of decision 0003.
//! `--try-bind-hidden INTERFACE`: binds the registry name right after the
//! last advertised one as INTERFACE (what a malicious client guessing a
//! hidden global would do) and expects the connection to end with
//! wl_display.error invalid_object on the registry.
//!
//! Keys are decoded with the compositor's keymap and modifier state.
//! Lines are tagged `[COMPOSITOR] info: client: ...`.

mod client;
mod input;
mod text;
mod window;

use client::{Connection, Proxy, Req, Val};
use input::{Devices, Goal};
use std::process::ExitCode;
use std::time::Duration;
use wana_log::{error, info, Subsystem};
use wana_render::scene::{ACCENT, BORDER};
use wana_wayland::protocols::{wayland, xdg_shell};
use window::{Shell, Window};

pub(crate) const LOG: Subsystem = Subsystem::Compositor;
const WIDTH: i32 = 480;
const HEIGHT: i32 = 320;
/// The z-order test's back window: larger than any test output (so the
/// compositor clamps it to 0,0 and it covers the screen), amber.
const BACK_SIZE: (i32, i32) = (1400, 900);
pub const BACK_FILL: u32 = 0xE0A030;
/// The z-order test's cursor image: a magenta square.
const CURSOR_SIZE: i32 = 64;
const CURSOR_RGB: u32 = 0xFF00FF;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Window,
    AttachBeforeConfigure,
    TruncatePool,
    Input(String),
    ZOrder(String),
    Text,
    /// (global, expected visible)
    Globals(Vec<(String, bool)>),
    TryBindHidden(String),
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    let mut mode = Mode::Window;
    let mut hold = 0u64;
    let mut fonts_dir = std::path::PathBuf::from(text::DEFAULT_DIR);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--attach-before-configure" => mode = Mode::AttachBeforeConfigure,
            "--truncate-pool" => mode = Mode::TruncatePool,
            "--text" => mode = Mode::Text,
            "--try-bind-hidden" => match it.next() {
                Some(iface) => mode = Mode::TryBindHidden(iface),
                None => {
                    error!(LOG, "client: --try-bind-hidden needs an interface name");
                    return ExitCode::from(2);
                }
            },
            "--expect-global" | "--expect-no-global" => {
                let Some(name) = it.next() else {
                    error!(LOG, "client: {a} needs a global name");
                    return ExitCode::from(2);
                };
                let visible = a == "--expect-global";
                match &mut mode {
                    Mode::Globals(v) => v.push((name, visible)),
                    _ => mode = Mode::Globals(vec![(name, visible)]),
                }
            }
            "--fonts" => match it.next() {
                Some(d) => fonts_dir = d.into(),
                None => {
                    error!(LOG, "client: --fonts needs a directory");
                    return ExitCode::from(2);
                }
            },
            "--hold" => hold = it.next().and_then(|v| v.parse().ok()).unwrap_or(0),
            "--input" | "--zorder" => {
                let Some(text) = it.next() else {
                    error!(LOG, "client: {a} needs the text to expect");
                    return ExitCode::from(2);
                };
                mode = if a == "--input" {
                    Mode::Input(text)
                } else {
                    Mode::ZOrder(text)
                };
            }
            other => {
                error!(LOG, "client: unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    match run(&mode, hold, &fonts_dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(LOG, "client: {e}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Default)]
struct Globals {
    compositor: Option<(u32, u32)>,
    shm: Option<(u32, u32)>,
    wm_base: Option<(u32, u32)>,
    seat: Option<(u32, u32)>,
}

fn run(mode: &Mode, hold: u64, fonts_dir: &std::path::Path) -> Result<(), String> {
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
    let mut g = Globals::default();
    let mut seen: Vec<(String, u32)> = Vec::new();
    let mut seen_names: Vec<u32> = Vec::new();
    while let Some(ev) = conn.next_event() {
        if ev.target == registry && ev.opcode == wayland::wl_registry::event::GLOBAL {
            if let [Val::Uint(name), Val::Str(iface), Val::Uint(version)] = &ev.args[..] {
                seen.push((iface.clone(), *version));
                seen_names.push(*name);
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
    if let Mode::Globals(expect) = mode {
        return check_globals(&seen, expect);
    }
    if let Mode::TryBindHidden(iface) = mode {
        return try_bind_hidden(&conn, registry, &seen_names, iface);
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
    let shell = Shell {
        compositor: bind(
            g.compositor,
            &wayland::WL_COMPOSITOR_INTERFACE,
            4,
            "wl_compositor",
        )?,
        shm: bind(g.shm, &wayland::WL_SHM_INTERFACE, 1, "wl_shm")?,
        wm_base: bind(
            g.wm_base,
            &xdg_shell::XDG_WM_BASE_INTERFACE,
            1,
            "xdg_wm_base",
        )?,
    };
    let wm_base = shell.wm_base;
    info!(LOG, "client: bound wl_compositor, wl_shm, xdg_wm_base");

    // Input objects exist before any window maps, so no focus event is lost.
    let mut devices = match mode {
        Mode::Input(_) | Mode::ZOrder(_) => {
            let seat = bind(g.seat, &wayland::WL_SEAT_INTERFACE, 7, "wl_seat")?;
            Some(Devices::new(&conn, seat)?)
        }
        _ => None,
    };

    let mut windows = Vec::new();
    if let Mode::ZOrder(_) = mode {
        let cursor = window::solid_surface(&conn, &shell, CURSOR_SIZE, CURSOR_RGB)?;
        if let Some(d) = devices.as_mut() {
            d.use_cursor(cursor);
        }
        let back = Window::new(
            &conn,
            &shell,
            "back",
            "wana-wl-test back",
            BACK_SIZE.0,
            BACK_SIZE.1,
            BACK_FILL,
            BORDER,
        )?;
        if let Some(d) = devices.as_mut() {
            d.add_window(back.surface, back.name);
        }
        back.configure(&conn, wm_base, devices.as_mut())?;
        back.present(&conn, wm_base, devices.as_mut())?;
        windows.push(back);
    }
    let name = match mode {
        Mode::ZOrder(_) => "front",
        _ => "main",
    };
    let win = if *mode == Mode::Text {
        let (canvas, (sx, sy)) = text::render(fonts_dir)?;
        info!(
            LOG,
            "client: fully inked pixel at {sx},{sy} (window coordinates)"
        );
        Window::with_pixels(
            &conn,
            &shell,
            "text",
            "wana-wl-test text",
            canvas.width as i32,
            canvas.height as i32,
            &canvas.bytes(),
        )?
    } else {
        Window::new(
            &conn,
            &shell,
            name,
            "wana-wl-test",
            WIDTH,
            HEIGHT,
            ACCENT,
            BORDER,
        )?
    };
    if let Some(d) = devices.as_mut() {
        d.add_window(win.surface, win.name);
    }

    if *mode == Mode::AttachBeforeConfigure {
        info!(
            LOG,
            "client: attaching a buffer before the first configure (protocol violation on purpose)"
        );
        win.attach_commit(&conn, false)?;
        return expect_error(&conn, "xdg_surface", 3);
    }
    win.configure(&conn, wm_base, devices.as_mut())?;
    if *mode == Mode::TruncatePool {
        win.file.set_len(0).map_err(|e| format!("ftruncate: {e}"))?;
        info!(LOG, "client: pool file truncated to 0 bytes; committing the buffer (protocol violation on purpose)");
        win.attach_commit(&conn, false)?;
        return expect_error(&conn, "wl_buffer", 2);
    }
    win.present(&conn, wm_base, devices.as_mut())?;
    windows.push(win);

    if let Some(dev) = devices.as_mut() {
        let (first_focus, goal) = match mode {
            Mode::Input(text) => (
                "main",
                Goal {
                    pointer: "main",
                    click: "main",
                    focus: "main",
                    text: text.clone(),
                },
            ),
            Mode::ZOrder(text) => (
                "front",
                Goal {
                    pointer: "back",
                    click: "back",
                    focus: "back",
                    text: text.clone(),
                },
            ),
            _ => unreachable!("devices only exist in the input modes"),
        };
        dev.wait_for_input(&conn, wm_base, first_focus, &goal)?;
    }
    if hold > 0 {
        info!(LOG, "client: holding window for {hold}s");
        std::thread::sleep(Duration::from_secs(hold));
    }
    for w in windows.iter().rev() {
        w.destroy(&conn);
    }
    conn.roundtrip()?;
    info!(LOG, "client: done");
    Ok(())
}

/// Binds the first registry name after the advertised ones as `iface`.
fn try_bind_hidden(
    conn: &Connection,
    registry: Proxy,
    names: &[u32],
    iface: &str,
) -> Result<(), String> {
    let table: &'static wana_wayland::sys::wl_interface = match iface {
        "zwlr_layer_shell_v1" => {
            &wana_wayland::protocols::wlr_layer_shell_unstable_v1::ZWLR_LAYER_SHELL_V1_INTERFACE
        }
        other => return Err(format!("--try-bind-hidden: unknown interface {other}")),
    };
    let name = names.iter().max().copied().unwrap_or(0) + 1;
    info!(
        LOG,
        "client: binding hidden global name {name} as {iface} (not advertised to this client)"
    );
    conn.request(
        registry,
        wayland::wl_registry::request::BIND,
        Some((table, 1)),
        &[Req::Uint(name), Req::Str(iface), Req::Uint(1), Req::NewId],
    )?;
    expect_error(conn, "wl_registry", 0)
}

/// Checks the advertised globals against `expect` (name, visible).
fn check_globals(seen: &[(String, u32)], expect: &[(String, bool)]) -> Result<(), String> {
    let mut wrong = Vec::new();
    for (name, visible) in expect {
        let found = seen.iter().find(|(n, _)| n == name);
        match (found, visible) {
            (Some((_, v)), true) => info!(LOG, "client: global {name} v{v} visible, as expected"),
            (None, false) => info!(LOG, "client: global {name} not visible, as expected"),
            (Some((_, v)), false) => wrong.push(format!("{name} v{v} is visible")),
            (None, true) => wrong.push(format!("{name} is not visible")),
        }
    }
    if wrong.is_empty() {
        info!(
            LOG,
            "client: globals as expected ({} advertised)",
            seen.len()
        );
        Ok(())
    } else {
        Err(format!("globals: {}", wrong.join("; ")))
    }
}

/// Dispatches events until `pick` returns a value; answers pings.
/// Every event also goes to `devices` (input can arrive at any time, e.g.
/// the keymap and keyboard enter while waiting for a configure). Events
/// already queued are handled before blocking for more.
pub(crate) fn wait_for<T>(
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
