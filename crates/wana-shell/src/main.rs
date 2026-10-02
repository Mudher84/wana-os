//! wana-shell: the Wana OS desktop shell (Phase 11, decision 0003).
//!
//! A privileged Wayland client: wana-compositor starts it on a private
//! connection (`WAYLAND_SOCKET`), the only client that sees
//! `zwlr_layer_shell_v1`. It draws:
//! - the desktop: a background layer surface on the whole output;
//! - the top bar: a top layer surface reserving its height (exclusive
//!   zone), RTL, with "وانا" at the start and the time at the end
//!   (Arabic-Indic digits, UTC until time zones are configurable), redrawn
//!   every minute;
//! - the Dock: a bottom layer surface fed by ext-foreign-toplevel-list-v1,
//!   showing the currently mapped application windows;
//! - the launcher: a click on the bar's start ("وانا") opens an overlay
//!   panel listing the apps of a pinned file (`apps.rs`). It holds the
//!   keyboard exclusively while open: Up/Down/Home/End select, Enter (or a
//!   click on a row) starts the app, Escape closes it.
//!
//! Programs it starts (`--autostart`, the launcher's apps) connect through
//! the public socket (`WAYLAND_DISPLAY`) like any application: they never
//! inherit the shell's privileged connection.
//!
//! Usage: wana-shell [--fonts DIR] [--clock HH:MM] [--apps FILE]
//!                   [--autostart PROGRAM [--autostart-arg ARG]... [--exit-with-autostart]]
//!                   [--exit-with-launched] [--test-launch N]
//!
//! `--clock` fixes the time shown (tests compare the bar's pixels by hash);
//! `--exit-with-autostart` / `--exit-with-launched` end the shell when the
//! autostarted program / the first app started from the launcher exits,
//! with its result (boot tests). `--test-launch N` opens the launcher once
//! ready and starts app N (from 1) as soon as it is shown, as Enter would
//! (for hosts without input devices).

mod apps;
mod draw;
mod launcher;
mod toplevels;

use apps::App;
use launcher::{Action, Menu};
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use toplevels::Toplevels;
use wana_client::client::{Connection, Event, Proxy, Req, Val};
use wana_client::layer::{self, LayerSurface, Spec};
use wana_client::shm::Buffer;
use wana_auth::AuthStatus;
use wana_input::keyboard::{Keyboard, Modifiers};
use wana_log::{debug, error, info, warn, Subsystem};
use wana_motion::{duration, Curve, Motion};
use wana_text::font::Font;
use wana_text::layout::FontSet;
use wana_text::raster::Canvas;
use wana_text::{fonts, sha256};
use wana_wayland::protocols::{
    ext_foreign_toplevel_list_v1 as foreign, wana_shell_control_v1 as shell_control, wayland,
    wlr_layer_shell_unstable_v1 as proto,
};

const SHELL: Subsystem = Subsystem::Shell;
const F_SETFD: i32 = 2;
const FD_CLOEXEC: i32 = 1;
const BTN_LEFT: u32 = 0x110;
const CAP_POINTER: u32 = 1;
const CAP_KEYBOARD: u32 = 2;
const LAUNCHER_SELECTION_MOTION: Motion = Motion::new(duration::QUICK_MS, 6, Curve::EaseOutCubic);
const KEY_BACKSPACE: u32 = 14;
const KEY_ENTER: u32 = 28;


extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

struct Args {
    fonts: PathBuf,
    clock: Option<String>,
    apps: PathBuf,
    /// Program and arguments.
    autostart: Option<Vec<String>>,
    exit_with_autostart: bool,
    exit_with_launched: bool,
    /// App to start (from 1) once the launcher is first shown.
    test_launch: Option<usize>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        fonts: fonts::DEFAULT_DIR.into(),
        clock: None,
        apps: apps::DEFAULT_FILE.into(),
        autostart: None,
        exit_with_autostart: false,
        exit_with_launched: false,
        test_launch: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |n: &str| it.next().ok_or(format!("{n} needs a value"));
        match arg.as_str() {
            "--fonts" => a.fonts = val("--fonts")?.into(),
            "--apps" => a.apps = val("--apps")?.into(),
            "--clock" => {
                let v = val("--clock")?;
                let ok = v.len() == 5
                    && v.as_bytes()[2] == b':'
                    && v[..2].parse::<u32>().is_ok_and(|h| h < 24)
                    && v[3..].parse::<u32>().is_ok_and(|m| m < 60);
                if !ok {
                    return Err(format!("--clock {v:?}: expected HH:MM"));
                }
                a.clock = Some(v);
            }
            "--autostart" => a.autostart = Some(vec![val("--autostart")?]),
            "--autostart-arg" => {
                let v = val("--autostart-arg")?;
                a.autostart
                    .as_mut()
                    .ok_or("--autostart-arg must follow --autostart")?
                    .push(v);
            }
            "--exit-with-autostart" => a.exit_with_autostart = true,
            "--exit-with-launched" => a.exit_with_launched = true,
            "--test-launch" => {
                let v = val("--test-launch")?;
                a.test_launch = Some(
                    v.parse()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or(format!("--test-launch {v:?}: expected a number from 1"))?,
                );
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

fn main() -> ExitCode {
    wana_log::init_from_env();
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            error!(SHELL, "{e}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(SHELL, "{e}");
            ExitCode::FAILURE
        }
    }
}

/// The time to show: fixed by `--clock`, else now (UTC).
fn time_now(args: &Args) -> String {
    if let Some(c) = &args.clock {
        return c.clone();
    }
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (h, m) = draw::clock(secs);
    format!("{h:02}:{m:02}")
}

/// Commits `canvas` to `surface`, converting the XRGB pixels to bytes
/// exactly once. The compositor copies at commit, so the buffer can be
/// destroyed immediately.
fn present(
    conn: &Connection,
    shm: Proxy,
    surface: Proxy,
    canvas: &Canvas,
    frame: bool,
) -> Result<(Vec<u8>, Option<Proxy>), String> {
    let bytes = canvas.bytes();
    let b = Buffer::new(conn, shm, canvas.width as i32, canvas.height as i32, &bytes)?;
    let cb = if frame {
        conn.request(
            surface,
            wayland::wl_surface::request::FRAME,
            Some((&wayland::WL_CALLBACK_INTERFACE, 1)),
            &[Req::NewId],
        )?
    } else {
        None
    };
    b.commit_to(conn, surface)?;
    b.destroy(conn);
    Ok((bytes, cb))
}

/// Shows a frame and returns its deterministic pixel hash when callers need
/// rendering evidence.
fn show(
    conn: &Connection,
    shm: Proxy,
    surface: Proxy,
    canvas: &Canvas,
    frame: bool,
) -> Result<(String, Option<Proxy>), String> {
    let (bytes, cb) = present(conn, shm, surface, canvas, frame)?;
    Ok((sha256::hex(&sha256::digest(&bytes)), cb))
}

/// Shows a transient frame without paying for a SHA-256 that nobody consumes.
fn show_unhashed(
    conn: &Connection,
    shm: Proxy,
    surface: Proxy,
    canvas: &Canvas,
) -> Result<(), String> {
    present(conn, shm, surface, canvas, false).map(|_| ())
}

/// The open launcher.
struct LockScreen {
    ls: LayerSurface,
    setup_required: bool,
    confirming: bool,
    first: Option<String>,
    input: String,
    error: Option<String>,
}

struct Launcher {
    ls: LayerSurface,
    menu: Menu,
    /// Selection before the latest navigation event, for the motion frame.
    previous_selected: usize,
    /// Frame callback of the first buffer (logged when presented).
    first_frame: Option<Proxy>,
    first_hash: String,
}

/// A program the shell started.
struct Started {
    name: String,
    child: Child,
    /// Its exit ends the shell (with its result).
    ends_shell: bool,
}

/// The seat as the shell uses it: clicks on its surfaces, keys on the
/// launcher.
#[derive(Default)]
struct Devices {
    pointer: Option<Proxy>,
    keyboard: Option<Proxy>,
    pointer_on: Option<Proxy>,
    at: (f64, f64),
    keyboard_on: Option<Proxy>,
    keymap: Option<Keyboard>,
}

/// What an input event asks of the shell.
enum Input {
    /// Left button pressed on `surface` at surface-local `x`, `y`.
    Click(Proxy, f64, f64),
    /// Key pressed while `surface` has the keyboard.
    Key(Proxy, u32, String),
}

impl Devices {
    fn event(
        &mut self,
        conn: &Connection,
        seat: Proxy,
        ev: &Event,
    ) -> Result<Option<Input>, String> {
        use wayland::{wl_keyboard::event as kev, wl_pointer::event as pev};
        let fixed = |v: &Val| match v {
            Val::Int(f) => f64::from(*f) / 256.0,
            _ => 0.0,
        };
        let surface = |v: &Val| match v {
            Val::Object(p) => *p,
            _ => None,
        };
        if ev.target == seat && ev.opcode == wayland::wl_seat::event::CAPABILITIES {
            let caps = match ev.args.first() {
                Some(Val::Uint(c)) => *c,
                _ => 0,
            };
            if caps & CAP_POINTER != 0 && self.pointer.is_none() {
                self.pointer = conn.request(
                    seat,
                    wayland::wl_seat::request::GET_POINTER,
                    Some((&wayland::WL_POINTER_INTERFACE, 7)),
                    &[Req::NewId],
                )?;
            }
            if caps & CAP_KEYBOARD != 0 && self.keyboard.is_none() {
                self.keyboard = conn.request(
                    seat,
                    wayland::wl_seat::request::GET_KEYBOARD,
                    Some((&wayland::WL_KEYBOARD_INTERFACE, 7)),
                    &[Req::NewId],
                )?;
            }
            debug!(SHELL, "seat capabilities {caps:#x}");
        } else if Some(ev.target) == self.keyboard {
            match (ev.opcode, &ev.args[..]) {
                (kev::KEYMAP, [_, Val::Int(fd), Val::Uint(size)]) => {
                    // SAFETY: the fd came with the event and ownership is transferred
                    // to the client by the Wayland protocol.
                    let file = unsafe { File::from_raw_fd(*fd) };
                    let mut text = String::new();
                    file.take(u64::from(*size))
                        .read_to_string(&mut text)
                        .map_err(|e| format!("read keyboard keymap: {e}"))?;
                    let text = text.trim_end_matches('\0');
                    self.keymap = Some(Keyboard::from_string(text)?);
                }
                (kev::ENTER, [_, s, _]) => self.keyboard_on = surface(s),
                (kev::LEAVE, [_, s]) if surface(s) == self.keyboard_on => self.keyboard_on = None,
                (
                    kev::MODIFIERS,
                    [
                        _,
                        Val::Uint(depressed),
                        Val::Uint(latched),
                        Val::Uint(locked),
                        Val::Uint(group),
                    ],
                ) => {
                    if let Some(keymap) = self.keymap.as_mut() {
                        keymap.set_modifiers(&Modifiers {
                            depressed: *depressed,
                            latched: *latched,
                            locked: *locked,
                            group: *group,
                        });
                    }
                }
                (kev::KEY, [_, _, Val::Uint(key), Val::Uint(state)]) => {
                    let pressed = *state == 1;
                    let text = self
                        .keymap
                        .as_mut()
                        .map(|keyboard| keyboard.key(*key, pressed).text)
                        .unwrap_or_default();
                    if pressed {
                        if let Some(s) = self.keyboard_on {
                            return Ok(Some(Input::Key(s, *key, text)));
                        }
                    }
                }
                _ => {}
            }
        } else if Some(ev.target) == self.pointer {
            match (ev.opcode, &ev.args[..]) {
                (pev::ENTER, [_, s, x, y]) => {
                    self.pointer_on = surface(s);
                    self.at = (fixed(x), fixed(y));
                }
                (pev::LEAVE, [_, s]) if surface(s) == self.pointer_on => self.pointer_on = None,
                (pev::MOTION, [_, x, y]) => self.at = (fixed(x), fixed(y)),
                (pev::BUTTON, [_, _, Val::Uint(BTN_LEFT), Val::Uint(1)]) => {
                    if let Some(s) = self.pointer_on {
                        return Ok(Some(Input::Click(s, self.at.0, self.at.1)));
                    }
                }
                _ => {}
            }
        }
        Ok(None)
    }
}

fn prepare_lock_socket() -> Result<Option<UnixDatagram>, String> {
    if !require_auth() {
        return Ok(None);
    }

    let path = std::path::Path::new(wana_auth::LOCK_SOCKET_PATH);
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.file_type().is_socket() || meta.uid() != 1000 {
                return Err(format!(
                    "{}: refusing to replace unsafe lock endpoint",
                    path.display()
                ));
            }
            std::fs::remove_file(path)
                .map_err(|e| format!("remove stale {}: {e}", path.display()))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", path.display())),
    }

    let socket = UnixDatagram::bind(path)
        .map_err(|e| format!("bind {}: {e}", path.display()))?;
    socket
        .set_nonblocking(true)
        .map_err(|e| format!("nonblocking {}: {e}", path.display()))?;
    info!(SHELL, "desktop lock endpoint ready: {}", path.display());
    Ok(Some(socket))
}

fn lock_requested(socket: &UnixDatagram) -> Result<bool, String> {
    let mut buffer = [0u8; 16];
    loop {
        match socket.recv(&mut buffer) {
            Ok(size) => {
                if &buffer[..size] == b"lock" {
                    return Ok(true);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(e) => return Err(format!("read desktop lock request: {e}")),
        }
    }
}

fn require_auth() -> bool {
    std::env::var("WANA_REQUIRE_AUTH").is_ok_and(|value| value == "1")
}

fn open_lock(
    conn: &Connection,
    compositor: Proxy,
    layer_shell: Proxy,
    shm: Proxy,
    fonts: &FontSet,
    events: &mut Vec<Event>,
) -> Result<LockScreen, String> {
    let setup_required = matches!(wana_auth::status()?, AuthStatus::SetupRequired);
    let ls = LayerSurface::new(
        conn,
        compositor,
        layer_shell,
        Spec {
            namespace: "wana-auth",
            layer: layer::OVERLAY,
            anchor: layer::ANCHOR_TOP
                | layer::ANCHOR_BOTTOM
                | layer::ANCHOR_LEFT
                | layer::ANCHOR_RIGHT,
            width: 0,
            height: 0,
            zone: 0,
            keyboard: layer::KEYBOARD_EXCLUSIVE,
        },
        events,
    )?;
    let lock = LockScreen {
        ls,
        setup_required,
        confirming: false,
        first: None,
        input: String::new(),
        error: None,
    };
    redraw_lock(conn, shm, fonts, &lock)?;
    info!(
        SHELL,
        "authentication screen mapped: mode={}",
        if setup_required { "setup" } else { "login" }
    );
    Ok(lock)
}

fn redraw_lock(
    conn: &Connection,
    shm: Proxy,
    fonts: &FontSet,
    lock: &LockScreen,
) -> Result<(), String> {
    show_unhashed(
        conn,
        shm,
        lock.ls.surface,
        &draw::auth(
            lock.ls.width,
            lock.ls.height,
            fonts,
            lock.setup_required,
            lock.confirming,
            lock.input.chars().count(),
            lock.error.as_deref(),
        )?,
    )
}

fn close_lock(conn: &Connection, lock: LockScreen) {
    conn.destroy(
        lock.ls.layer_surface,
        proto::zwlr_layer_surface_v1::request::DESTROY,
    );
    conn.destroy(lock.ls.surface, wayland::wl_surface::request::DESTROY);
}

fn append_auth_text(lock: &mut LockScreen, text: &str) {
    for ch in text.chars() {
        if ch.is_control() {
            continue;
        }
        let extra = ch.len_utf8();
        if lock.input.len() + extra > wana_auth::MAX_PASSWORD_BYTES {
            break;
        }
        lock.input.push(ch);
    }
}

fn auth_key(lock: &mut LockScreen, key: u32, text: &str) -> Result<bool, String> {
    lock.error = None;
    match key {
        KEY_BACKSPACE => {
            lock.input.pop();
            Ok(false)
        }
        KEY_ENTER if lock.setup_required => {
            if let Err(e) = wana_auth::validate_password(&lock.input) {
                lock.input.clear();
                lock.error = Some(e);
                return Ok(false);
            }
            if !lock.confirming {
                lock.first = Some(std::mem::take(&mut lock.input));
                lock.confirming = true;
                info!(SHELL, "authentication confirmation requested");
                return Ok(false);
            }
            let Some(first) = lock.first.take() else {
                lock.confirming = false;
                lock.input.clear();
                lock.error = Some("أعد إنشاء كلمة المرور".into());
                return Ok(false);
            };
            if first != lock.input {
                lock.input.clear();
                lock.confirming = false;
                lock.error = Some("كلمتا المرور غير متطابقتين".into());
                return Ok(false);
            }
            wana_auth::setup(&lock.input)?;
            lock.input.clear();
            info!(SHELL, "initial desktop credential setup complete");
            Ok(true)
        }
        KEY_ENTER => {
            let ok = wana_auth::verify(&lock.input)?;
            lock.input.clear();
            if ok {
                info!(SHELL, "desktop credential accepted");
                Ok(true)
            } else {
                lock.error = Some("كلمة المرور غير صحيحة".into());
                Ok(false)
            }
        }
        _ => {
            if !text.is_empty() {
                append_auth_text(lock, text);
            }
            Ok(false)
        }
    }
}
fn run(args: &Args) -> Result<(), String> {
    info!(SHELL, "wana-shell {} starting", env!("CARGO_PKG_VERSION"));
    // libwayland reads WAYLAND_SOCKET (and unsets it); keep that privileged
    // descriptor out of every program the shell starts.
    let private_fd: Option<i32> = std::env::var("WAYLAND_SOCKET")
        .ok()
        .and_then(|v| v.parse().ok());
    let conn = Connection::connect()?;
    if let Some(fd) = private_fd {
        // SAFETY: fcntl on a descriptor we own; no memory involved.
        // SAFETY: fd is from /proc/self/fd; F_SETFD does not dereference memory.
        if unsafe { fcntl(fd, F_SETFD, FD_CLOEXEC) } < 0 {
            return Err(format!(
                "private connection fd {fd}: {}",
                std::io::Error::last_os_error()
            ));
        }
    }

    // Globals: the layer shell is visible only to the real shell.
    let registry = conn
        .request(
            conn.display(),
            wayland::wl_display::request::GET_REGISTRY,
            Some((&wayland::WL_REGISTRY_INTERFACE, 1)),
            &[Req::NewId],
        )?
        .ok_or("no registry")?;
    conn.roundtrip()?;
    let mut globals: Vec<(u32, String, u32)> = Vec::new();
    while let Some(ev) = conn.next_event() {
        if ev.target == registry && ev.opcode == wayland::wl_registry::event::GLOBAL {
            if let [Val::Uint(n), Val::Str(i), Val::Uint(v)] = &ev.args[..] {
                globals.push((*n, i.clone(), *v));
            }
        }
    }
    let bind = |name: &str, iface: &'static wana_wayland::sys::wl_interface, want: u32| {
        let (id, ver) = globals
            .iter()
            .find(|g| g.1 == name)
            .map(|g| (g.0, g.2))
            .ok_or_else(|| format!("{name} not advertised"))?;
        let v = ver.min(want);
        conn.request(
            registry,
            wayland::wl_registry::request::BIND,
            Some((iface, v)),
            &[Req::Uint(id), Req::Str(name), Req::Uint(v), Req::NewId],
        )?
        .ok_or_else(|| format!("bind {name}"))
    };
    let compositor = bind("wl_compositor", &wayland::WL_COMPOSITOR_INTERFACE, 4)?;
    let shm = bind("wl_shm", &wayland::WL_SHM_INTERFACE, 1)?;
    let layer_shell = bind(
        "zwlr_layer_shell_v1",
        &proto::ZWLR_LAYER_SHELL_V1_INTERFACE,
        4,
    )
    .map_err(|e| format!("{e}: not started as the shell by wana-compositor?"))?;
    let seat = bind("wl_seat", &wayland::WL_SEAT_INTERFACE, 7)?;
    let foreign_list = bind(
        "ext_foreign_toplevel_list_v1",
        &foreign::EXT_FOREIGN_TOPLEVEL_LIST_V1_INTERFACE,
        1,
    )
    .map_err(|e| format!("{e}: foreign toplevel list is restricted to wana-shell"))?;
    let mut toplevels = Toplevels::new(foreign_list);
    let shell_control = bind(
        "wana_shell_control_v1",
        &shell_control::WANA_SHELL_CONTROL_V1_INTERFACE,
        1,
    )
    .map_err(|e| format!("{e}: shell control is restricted to wana-shell"))?;

    fonts::verify_dir(&args.fonts)?;
    let set = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|n| Font::load(&args.fonts.join(n)))
            .collect::<Result<_, _>>()?,
    };
    // A missing or broken list leaves the launcher empty, not the desktop.
    let apps = apps::load(&args.apps).unwrap_or_else(|e| {
        warn!(SHELL, "apps: {e}");
        Vec::new()
    });
    info!(SHELL, "apps: {} from {}", apps.len(), args.apps.display());

    let mut events = Vec::new();
    let desk = LayerSurface::new(
        &conn,
        compositor,
        layer_shell,
        Spec {
            namespace: "wana-desktop",
            layer: layer::BACKGROUND,
            anchor: layer::ANCHOR_TOP
                | layer::ANCHOR_BOTTOM
                | layer::ANCHOR_LEFT
                | layer::ANCHOR_RIGHT,
            width: 0,
            height: 0,
            zone: -1,
            keyboard: layer::KEYBOARD_NONE,
        },
        &mut events,
    )?;
    let (hash, _) = show(
        &conn,
        shm,
        desk.surface,
        &draw::desktop(desk.width, desk.height),
        false,
    )?;
    info!(
        SHELL,
        "desktop mapped: {}x{}, sha256 {hash}", desk.width, desk.height
    );
    let bar = LayerSurface::new(
        &conn,
        compositor,
        layer_shell,
        Spec {
            namespace: "wana-bar",
            layer: layer::TOP,
            anchor: layer::ANCHOR_TOP | layer::ANCHOR_LEFT | layer::ANCHOR_RIGHT,
            width: 0,
            height: draw::BAR_HEIGHT,
            zone: draw::BAR_HEIGHT as i32,
            keyboard: layer::KEYBOARD_NONE,
        },
        &mut events,
    )?;
    let mut shown_time = time_now(args);
    let mut bar_width = bar.width;
    let (hash, _) = show(
        &conn,
        shm,
        bar.surface,
        &draw::bar(bar_width, &set, &shown_time)?,
        false,
    )?;
    info!(
        SHELL,
        "bar mapped: {}x{}, time {}, sha256 {hash}",
        bar.width,
        bar.height,
        draw::arabic_digits(&shown_time)
    );

    let dock = LayerSurface::new(
        &conn,
        compositor,
        layer_shell,
        Spec {
            namespace: "wana-dock",
            layer: layer::TOP,
            anchor: layer::ANCHOR_BOTTOM,
            width: draw::DOCK_WIDTH,
            height: draw::DOCK_HEIGHT,
            zone: 0,
            keyboard: layer::KEYBOARD_NONE,
        },
        &mut events,
    )?;
    let mut dock_width = dock.width;
    let (dock_hash, _) = show(
        &conn,
        shm,
        dock.surface,
        &draw::dock(dock_width, &set, &toplevels.labels())?,
        false,
    )?;
    info!(
        SHELL,
        "dock mapped: {}x{}, windows {}, sha256 {dock_hash}",
        dock.width,
        dock.height,
        toplevels.len()
    );

    let lock_socket = prepare_lock_socket()?;
    let mut lock = if require_auth() {
        Some(open_lock(
            &conn,
            compositor,
            layer_shell,
            shm,
            &set,
            &mut events,
        )?)
    } else {
        None
    };

    conn.roundtrip()?;
    if lock.is_some() {
        info!(SHELL, "locked: authentication required");
    } else {
        info!(SHELL, "ready");
    }

    let mut started: Vec<Started> = Vec::new();
    if lock.is_none() {
        if let Some(cmd) = &args.autostart {
            started.push(Started {
                name: "autostart".into(),
                child: spawn(cmd, "autostart")?,
                ends_shell: args.exit_with_autostart,
            });
        }
    }

    let mut devices = Devices::default();
    let mut launcher: Option<Launcher> = None;
    let mut test_launch = args.test_launch;
    if lock.is_none() && test_launch.is_some() {
        launcher = Some(open_launcher(
            &conn,
            compositor,
            layer_shell,
            shm,
            &set,
            &apps,
            &mut events,
        )?);
    }

    loop {
        if lock.is_none() {
            let requested = match &lock_socket {
                Some(socket) => lock_requested(socket)?,
                None => false,
            };
            if requested {
                close_launcher(&conn, &mut launcher);
                lock = Some(open_lock(
                    &conn,
                    compositor,
                    layer_shell,
                    shm,
                    &set,
                    &mut events,
                )?);
                info!(SHELL, "locked by user request");
            }
        }

        // Wake at least every second: the clock and the started programs.
        // Events queued while opening the launcher are handled at once.
        conn.wait(if events.is_empty() { 1000 } else { 0 })?;
        events.extend(std::iter::from_fn(|| conn.next_event()));
        let batch: Vec<Event> = std::mem::take(&mut events);
        for ev in batch {
            if let Some((serial, w, h)) = layer::configure_of(&ev, desk.layer_surface) {
                layer::ack(&conn, desk.layer_surface, serial)?;
                show(&conn, shm, desk.surface, &draw::desktop(w, h), false)?;
                info!(SHELL, "desktop resized to {w}x{h}");
                continue;
            }
            if let Some((serial, w, _)) = layer::configure_of(&ev, bar.layer_surface) {
                layer::ack(&conn, bar.layer_surface, serial)?;
                bar_width = w;
                show(
                    &conn,
                    shm,
                    bar.surface,
                    &draw::bar(bar_width, &set, &shown_time)?,
                    false,
                )?;
                continue;
            }
            if let Some((serial, w, _)) = layer::configure_of(&ev, dock.layer_surface) {
                layer::ack(&conn, dock.layer_surface, serial)?;
                dock_width = w;
                show(
                    &conn,
                    shm,
                    dock.surface,
                    &draw::dock(dock_width, &set, &toplevels.labels())?,
                    false,
                )?;
                continue;
            }
            if layer::closed(&ev, desk.layer_surface)
                || layer::closed(&ev, bar.layer_surface)
                || layer::closed(&ev, dock.layer_surface)
            {
                return Err("the compositor closed a shell surface".into());
            }
            if let Some(active) = lock.as_mut() {
                if let Some((serial, width, height)) =
                    layer::configure_of(&ev, active.ls.layer_surface)
                {
                    layer::ack(&conn, active.ls.layer_surface, serial)?;
                    show_unhashed(
                        &conn,
                        shm,
                        active.ls.surface,
                        &draw::auth(
                            width,
                            height,
                            &set,
                            active.setup_required,
                            active.confirming,
                            active.input.chars().count(),
                            active.error.as_deref(),
                        )?,
                    )?;
                    continue;
                }
                if layer::closed(&ev, active.ls.layer_surface) {
                    return Err("the compositor closed the authentication surface".into());
                }
            }

            let input = devices.event(&conn, seat, &ev)?;
            let mut auth_unlocked = false;
            if let Some(active) = lock.as_mut() {
                if let Some(Input::Key(surface, key, text)) = input.as_ref() {
                    if *surface == active.ls.surface {
                        auth_unlocked = auth_key(active, *key, text)?;
                        if !auth_unlocked {
                            redraw_lock(&conn, shm, &set, active)?;
                        }
                    }
                }
            }
            if lock.is_some() {
                if auth_unlocked {
                    let finished = lock.take().expect("authentication lock exists");
                    close_lock(&conn, finished);
                    info!(SHELL, "authentication complete");
                    info!(SHELL, "ready");
                    if let Some(cmd) = &args.autostart {
                        started.push(Started {
                            name: "autostart".into(),
                            child: spawn(cmd, "autostart")?,
                            ends_shell: args.exit_with_autostart,
                        });
                    }
                    if test_launch.is_some() {
                        launcher = Some(open_launcher(
                            &conn,
                            compositor,
                            layer_shell,
                            shm,
                            &set,
                            &apps,
                            &mut events,
                        )?);
                    }
                }
                continue;
            }
            if toplevels.event(&conn, &ev)? {
                let labels = toplevels.labels();
                let (hash, _) = show(
                    &conn,
                    shm,
                    dock.surface,
                    &draw::dock(dock_width, &set, &labels)?,
                    false,
                )?;
                info!(
                    SHELL,
                    "dock updated: {} window(s), sha256 {hash}",
                    toplevels.len()
                );
                continue;
            }
            if ev.target == shell_control
                && ev.opcode == shell_control::wana_shell_control_v1::event::TOGGLE_LAUNCHER
            {
                if launcher.is_some() {
                    info!(SHELL, "launcher closed from Super");
                    close_launcher(&conn, &mut launcher);
                } else if apps.is_empty() {
                    warn!(SHELL, "launcher: no apps ({})", args.apps.display());
                } else {
                    info!(SHELL, "launcher opened from Super");
                    launcher = Some(open_launcher(
                        &conn,
                        compositor,
                        layer_shell,
                        shm,
                        &set,
                        &apps,
                        &mut events,
                    )?);
                }
                continue;
            }

            let mut action = Action::None;
            if let Some(l) = launcher.as_mut() {
                if let Some((serial, _, _)) = layer::configure_of(&ev, l.ls.layer_surface) {
                    layer::ack(&conn, l.ls.layer_surface, serial)?;
                    continue;
                }
                if layer::closed(&ev, l.ls.layer_surface) {
                    action = Action::Close;
                } else if Some(ev.target) == l.first_frame
                    && ev.opcode == wayland::wl_callback::event::DONE
                {
                    l.first_frame = None;
                    info!(
                        SHELL,
                        "launcher shown: {}x{}, {}, sha256 {}",
                        l.ls.width,
                        l.ls.height,
                        selection(&apps, &l.menu),
                        l.first_hash
                    );
                    if let Some(n) = test_launch.take() {
                        action = if n <= apps.len() {
                            Action::Launch(n - 1)
                        } else {
                            return Err(format!("--test-launch {n}: only {} app(s)", apps.len()));
                        };
                    }
                }
            }
            match input {
                Some(Input::Click(s, x, _)) if s == bar.surface => {
                    // The bar's start is its right end (RTL).
                    if x >= f64::from(bar_width.saturating_sub(draw::BRAND_HIT)) {
                        if launcher.is_some() {
                            action = Action::Close;
                        } else if apps.is_empty() {
                            warn!(SHELL, "launcher: no apps ({})", args.apps.display());
                        } else {
                            info!(SHELL, "launcher opened from the bar");
                            launcher = Some(open_launcher(
                                &conn,
                                compositor,
                                layer_shell,
                                shm,
                                &set,
                                &apps,
                                &mut events,
                            )?);
                        }
                    }
                }
                Some(Input::Click(s, _, y))
                    if launcher.as_ref().is_some_and(|l| l.ls.surface == s) =>
                {
                    if let Some(row) = draw::launcher_row_at(y, apps.len()) {
                        action = Action::Launch(row);
                    }
                }
                Some(Input::Key(s, key, _)) => {
                    if let Some(l) = launcher.as_mut().filter(|l| l.ls.surface == s) {
                        let before = l.menu.selected;
                        action = l.menu.key(key);
                        if action == Action::Moved {
                            l.previous_selected = before;
                        }
                    }
                }
                _ => {}
            }
            match action {
                Action::None => {}
                Action::Moved => {
                    if let Some(l) = &launcher {
                        animate_launcher_selection(&conn, shm, &set, &apps, l)?;
                        info!(SHELL, "launcher: {}", selection(&apps, &l.menu));
                    }
                }
                Action::Launch(i) => {
                    close_launcher(&conn, &mut launcher);
                    let app = &apps[i];
                    let name = format!("app {:?}", app.name);
                    match spawn(&app.argv, &name) {
                        Ok(child) => started.push(Started {
                            name,
                            child,
                            ends_shell: args.exit_with_launched,
                        }),
                        // A broken entry must not take the desktop down.
                        Err(e) => warn!(SHELL, "{e}"),
                    }
                }
                Action::Close => close_launcher(&conn, &mut launcher),
            }
        }
        let now = time_now(args);
        if now != shown_time {
            shown_time = now;
            show(
                &conn,
                shm,
                bar.surface,
                &draw::bar(bar_width, &set, &shown_time)?,
                false,
            )?;
        }
        let mut i = 0;
        while i < started.len() {
            let Ok(Some(status)) = started[i].child.try_wait() else {
                i += 1;
                continue;
            };
            let s = started.remove(i);
            if status.success() {
                info!(SHELL, "{} exited successfully", s.name);
            } else {
                warn!(SHELL, "{} exited: {status}", s.name);
            }
            if s.ends_shell {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("{} failed: {status}", s.name))
                };
            }
        }
    }
}

/// "selected 2/3 "name"".
fn selection(apps: &[App], menu: &Menu) -> String {
    format!(
        "selected {}/{} {:?}",
        menu.selected + 1,
        apps.len(),
        apps.get(menu.selected).map_or("", |a| a.name.as_str())
    )
}

/// Maps the launcher: an overlay panel centered in the usable area,
/// holding the keyboard while it is open.
fn open_launcher(
    conn: &Connection,
    compositor: Proxy,
    layer_shell: Proxy,
    shm: Proxy,
    set: &FontSet,
    apps: &[App],
    events: &mut Vec<Event>,
) -> Result<Launcher, String> {
    let ls = LayerSurface::new(
        conn,
        compositor,
        layer_shell,
        Spec {
            namespace: "wana-launcher",
            layer: layer::OVERLAY,
            anchor: 0,
            width: draw::LAUNCHER_WIDTH,
            height: draw::launcher_height(apps.len()),
            zone: 0,
            keyboard: layer::KEYBOARD_EXCLUSIVE,
        },
        events,
    )?;
    let menu = Menu::new(apps.len());
    let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
    let (first_hash, first_frame) = show(
        conn,
        shm,
        ls.surface,
        &draw::launcher(set, &names, menu.selected)?,
        true,
    )?;
    Ok(Launcher {
        ls,
        menu,
        previous_selected: 0,
        first_frame,
        first_hash,
    })
}

fn animate_launcher_selection(
    conn: &Connection,
    shm: Proxy,
    set: &FontSet,
    apps: &[App],
    launcher: &Launcher,
) -> Result<(), String> {
    let names: Vec<&str> = apps.iter().map(|app| app.name.as_str()).collect();
    let motion = LAUNCHER_SELECTION_MOTION;
    for frame in 1..=motion.frames {
        let progress = motion.progress(frame);
        show_unhashed(
            conn,
            shm,
            launcher.ls.surface,
            &draw::launcher_transition(
                set,
                &names,
                launcher.previous_selected,
                launcher.menu.selected,
                progress,
            )?,
        )?;
        // wait() may return early when Wayland traffic arrives. Keep waiting
        // until the frame deadline so QUICK_MS is a real minimum duration,
        // while protocol/input events remain queued for the main loop.
        let deadline = Instant::now() + Duration::from_millis(u64::from(motion.frame_ms()));
        loop {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let remaining = deadline.saturating_duration_since(now);
            let timeout_ms = remaining.as_millis().clamp(1, i32::MAX as u128) as i32;
            conn.wait(timeout_ms)?;
        }
    }
    info!(
        SHELL,
        "motion launcher-selection: {} -> {}, {} frames, {}ms, ease-out-cubic",
        launcher.previous_selected + 1,
        launcher.menu.selected + 1,
        motion.frames,
        motion.duration_ms
    );
    Ok(())
}

fn close_launcher(conn: &Connection, launcher: &mut Option<Launcher>) {
    if let Some(l) = launcher.take() {
        conn.destroy(
            l.ls.layer_surface,
            proto::zwlr_layer_surface_v1::request::DESTROY,
        );
        conn.destroy(l.ls.surface, wayland::wl_surface::request::DESTROY);
        info!(SHELL, "launcher closed");
    }
}

/// Marks every non-stdio descriptor currently owned by the shell
/// close-on-exec. This includes descriptors inherited from the compositor or
/// CI/service manager as well as libwayland internals. Applications must start
/// with only 0, 1 and 2 inherited; they open their public Wayland connection
/// themselves after exec.
fn seal_child_fds() -> Result<(), String> {
    let entries = std::fs::read_dir("/proc/self/fd")
        .map_err(|e| format!("cannot enumerate shell descriptors: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("cannot enumerate shell descriptor: {e}"))?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(fd) = name.parse::<i32>() else {
            continue;
        };
        if fd <= 2 {
            continue;
        }
        // The /proc iterator itself may close a descriptor between listing
        // and this call; EBADF is harmless. Any other fcntl failure means we
        // cannot guarantee descriptor isolation for the child.
        let rc = {
            // SAFETY: fd is from /proc/self/fd; F_SETFD does not dereference memory.
            unsafe { fcntl(fd, F_SETFD, FD_CLOEXEC) }
        };
        if rc < 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() != Some(9) {
                return Err(format!("cannot mark fd {fd} close-on-exec: {e}"));
            }
        }
    }
    Ok(())
}

/// Starts a program as an ordinary client: public socket, clean
/// environment, with every shell-owned descriptor close-on-exec.
fn spawn(argv: &[String], what: &str) -> Result<Child, String> {
    seal_child_fds()?;
    let prog = argv.first().ok_or(format!("{what}: empty command"))?;
    let display = std::env::var("WAYLAND_DISPLAY")
        .map_err(|_| "WAYLAND_DISPLAY not set: cannot start programs".to_string())?;
    let mut c = Command::new(prog);
    c.args(&argv[1..])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("WAYLAND_DISPLAY", display);
    if let Some(home) = std::env::var_os("HOME") {
        c.env("HOME", home);
    }
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        c.env("XDG_RUNTIME_DIR", dir);
    }
    let child = c.spawn().map_err(|e| format!("{what}: {prog}: {e}"))?;
    info!(SHELL, "{what}: {} (pid {})", argv.join(" "), child.id());
    Ok(child)
}
