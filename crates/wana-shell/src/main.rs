//! wana-shell: the Wana OS desktop shell (Phase 11, decision 0003).
//!
//! A privileged Wayland client: wana-compositor starts it on a private
//! connection (`WAYLAND_SOCKET`), the only client that sees
//! `zwlr_layer_shell_v1`. It draws:
//! - the desktop: a background layer surface on the whole output;
//! - the top bar: a top layer surface reserving its height (exclusive
//!   zone), RTL, with "وانا" at the start and the time at the end
//!   (Arabic-Indic digits, UTC until time zones are configurable), redrawn
//!   every minute.
//!
//! Programs it starts (`--autostart`, later the launcher) connect through
//! the public socket (`WAYLAND_DISPLAY`) like any application: they never
//! inherit the shell's privileged connection.
//!
//! Usage: wana-shell [--fonts DIR] [--clock HH:MM]
//!                   [--autostart PROGRAM [--autostart-arg ARG]... [--exit-with-autostart]]
//!
//! `--clock` fixes the time shown (tests compare the bar's pixels by hash);
//! `--exit-with-autostart` ends the shell when the autostarted program
//! exits, with its result (boot tests).

mod draw;

use std::path::PathBuf;
use std::process::{Child, Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};
use wana_client::client::{Connection, Proxy, Req, Val};
use wana_client::layer::{self, LayerSurface, Spec};
use wana_client::shm::Buffer;
use wana_log::{error, info, warn, Subsystem};
use wana_text::font::Font;
use wana_text::layout::FontSet;
use wana_text::raster::Canvas;
use wana_text::{fonts, sha256};
use wana_wayland::protocols::{wayland, wlr_layer_shell_unstable_v1 as proto};

const SHELL: Subsystem = Subsystem::Shell;
const F_SETFD: i32 = 2;
const FD_CLOEXEC: i32 = 1;

extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

struct Args {
    fonts: PathBuf,
    clock: Option<String>,
    /// Program and arguments.
    autostart: Option<Vec<String>>,
    exit_with_autostart: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        fonts: fonts::DEFAULT_DIR.into(),
        clock: None,
        autostart: None,
        exit_with_autostart: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = |n: &str| it.next().ok_or(format!("{n} needs a value"));
        match arg.as_str() {
            "--fonts" => a.fonts = val("--fonts")?.into(),
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

/// Shows `canvas` on `surface` (one buffer per frame: the compositor copies
/// at commit, so the buffer is destroyed right after).
fn show(conn: &Connection, shm: Proxy, surface: Proxy, canvas: &Canvas) -> Result<String, String> {
    let b = Buffer::new(
        conn,
        shm,
        canvas.width as i32,
        canvas.height as i32,
        &canvas.bytes(),
    )?;
    b.commit_to(conn, surface)?;
    b.destroy(conn);
    Ok(sha256::hex(&sha256::digest(&canvas.bytes())))
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

    fonts::verify_dir(&args.fonts)?;
    let set = FontSet {
        fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
            .iter()
            .map(|n| Font::load(&args.fonts.join(n)))
            .collect::<Result<_, _>>()?,
    };

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
    let hash = show(
        &conn,
        shm,
        desk.surface,
        &draw::desktop(desk.width, desk.height),
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
    let hash = show(
        &conn,
        shm,
        bar.surface,
        &draw::bar(bar_width, &set, &shown_time)?,
    )?;
    info!(
        SHELL,
        "bar mapped: {}x{}, time {}, sha256 {hash}",
        bar.width,
        bar.height,
        draw::arabic_digits(&shown_time)
    );
    conn.roundtrip()?;
    info!(SHELL, "ready");

    let mut child = match &args.autostart {
        Some(cmd) => Some(autostart(cmd)?),
        None => None,
    };

    loop {
        // Wake at least every second: the clock and the autostarted child.
        conn.wait(1000)?;
        events.extend(std::iter::from_fn(|| conn.next_event()));
        for ev in events.drain(..) {
            if let Some((serial, w, h)) = layer::configure_of(&ev, desk.layer_surface) {
                layer::ack(&conn, desk.layer_surface, serial)?;
                show(&conn, shm, desk.surface, &draw::desktop(w, h))?;
                info!(SHELL, "desktop resized to {w}x{h}");
            } else if let Some((serial, w, _)) = layer::configure_of(&ev, bar.layer_surface) {
                layer::ack(&conn, bar.layer_surface, serial)?;
                bar_width = w;
                show(
                    &conn,
                    shm,
                    bar.surface,
                    &draw::bar(bar_width, &set, &shown_time)?,
                )?;
            } else if layer::closed(&ev, desk.layer_surface)
                || layer::closed(&ev, bar.layer_surface)
            {
                return Err("the compositor closed a shell surface".into());
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
            )?;
        }
        if let Some(c) = child.as_mut() {
            if let Ok(Some(status)) = c.try_wait() {
                if status.success() {
                    info!(SHELL, "autostart exited successfully");
                } else {
                    warn!(SHELL, "autostart exited: {status}");
                }
                child = None;
                if args.exit_with_autostart {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(format!("autostart failed: {status}"))
                    };
                }
            }
        }
    }
}

/// Starts a program as an ordinary client: public socket, clean
/// environment, no privileged descriptor (it is close-on-exec).
fn autostart(argv: &[String]) -> Result<Child, String> {
    let prog = argv.first().ok_or("--autostart: empty command")?;
    let display = std::env::var("WAYLAND_DISPLAY")
        .map_err(|_| "WAYLAND_DISPLAY not set: cannot start programs".to_string())?;
    let mut c = Command::new(prog);
    c.args(&argv[1..])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("WAYLAND_DISPLAY", display);
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        c.env("XDG_RUNTIME_DIR", dir);
    }
    let child = c.spawn().map_err(|e| format!("autostart {prog}: {e}"))?;
    info!(SHELL, "autostart: {} (pid {})", argv.join(" "), child.id());
    Ok(child)
}
