//! wana-compositor: the Wana OS Wayland compositor.
//!
//! Phase 10 scope so far:
//! - step 1: prepares `$XDG_RUNTIME_DIR`, opens the Wayland socket through
//!   libwayland-server, runs the event loop and logs every client
//!   connect/disconnect with its credentials;
//! - step 2: advertises `wl_compositor`, `wl_shm`, `wl_output`, `wl_seat`
//!   and `xdg_wm_base` (see `globals.rs`); `wl_output` describes the display
//!   found through wana-drm (`--headless WxH@HZ` for machines without one);
//! - step 3: client windows (wl_shm buffers, xdg_toplevel) composited with
//!   GLES and page-flipped on the display;
//! - step 4: input from libinput routed to the focused client through
//!   wl_seat (pointer, keyboard with the `--layout` xkb keymap), with a
//!   cursor. Headless runs have no input.
//!
//! `--run PROGRAM [ARGS...]` starts one client with `WAYLAND_DISPLAY` set,
//! serves it until it exits, then shuts down and returns its result, which
//! makes the compositor usable as a boot test (`wana.run=`).
//! `--client-debug` sets `WAYLAND_DEBUG=1` for that client so the console
//! shows the protocol messages it exchanges.
//!
//! `--shell PROGRAM [--shell-arg ARG]...` starts the shell on a private,
//! privileged connection (decision 0003, `shell.rs`).
//!
//! Usage: wana-compositor [--timeout SECONDS] [--headless WxH@HZ] [--layout us]
//!                        [--shell PROGRAM [--shell-arg ARG]... [--exit-with-shell]]
//!                        [--client-debug] [--run PROGRAM [ARGS...]]

mod globals;
mod input;
mod layer;
mod render;
mod seat;
mod shell;
mod shell_surfaces;
mod shm;
mod surface;

use globals::{Compositor, OutputInfo};
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode};
use std::time::{Duration, Instant};
use wana_input::keyboard::Keyboard;
use wana_input::libinput::Libinput;
use wana_log::{error, info, warn, Subsystem};
use wana_render::scene;
use wana_wayland::server::{interface_name, ClientEvent, Display};

const COMPOSITOR: Subsystem = Subsystem::Compositor;
const TICK: Duration = Duration::from_millis(100);
/// Headless stand-in for the refresh period (frame callbacks without a screen).
const HEADLESS_FRAME: Duration = Duration::from_millis(16);

struct Args {
    timeout: Option<u64>,
    layout: String,
    client_debug: bool,
    headless: Option<(i32, i32, i32)>,
    run: Option<Vec<String>>,
    /// The shell program and its arguments (`--shell`, `--shell-arg`).
    shell: Option<Vec<String>>,
    /// Stop when the shell exits, with its result (boot tests).
    exit_with_shell: bool,
}

/// `1280x800@60` -> (1280, 800, 60000 mHz).
fn parse_mode(v: &str) -> Result<(i32, i32, i32), String> {
    let bad = || format!("--headless {v:?}: expected WIDTHxHEIGHT@HZ");
    let (size, hz) = v.split_once('@').ok_or_else(bad)?;
    let (w, h) = size.split_once('x').ok_or_else(bad)?;
    let num = |s: &str| s.parse::<i32>().ok().filter(|n| *n > 0).ok_or_else(bad);
    let hz: f64 = hz.parse().map_err(|_| bad())?;
    Ok((num(w)?, num(h)?, (hz * 1000.0).round() as i32))
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        timeout: None,
        layout: "us".into(),
        client_debug: false,
        headless: None,
        run: None,
        shell: None,
        exit_with_shell: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--timeout" => {
                let v = it.next().ok_or("--timeout needs a value")?;
                a.timeout = Some(v.parse().map_err(|e| format!("--timeout: {e}"))?);
            }
            "--client-debug" => a.client_debug = true,
            "--exit-with-shell" => a.exit_with_shell = true,
            "--shell" => a.shell = Some(vec![it.next().ok_or("--shell needs a program")?]),
            "--shell-arg" => {
                let v = it.next().ok_or("--shell-arg needs a value")?;
                a.shell
                    .as_mut()
                    .ok_or("--shell-arg must follow --shell")?
                    .push(v);
            }
            "--layout" => a.layout = it.next().ok_or("--layout needs a value")?,
            "--headless" => {
                let v = it.next().ok_or("--headless needs WIDTHxHEIGHT@HZ")?;
                a.headless = Some(parse_mode(&v)?);
            }
            "--run" => {
                let argv: Vec<String> = it.by_ref().collect();
                if argv.is_empty() {
                    return Err("--run needs a program".into());
                }
                a.run = Some(argv);
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
            error!(COMPOSITOR, "{e}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(COMPOSITOR, "{e}");
            ExitCode::FAILURE
        }
    }
}

/// Uses `$XDG_RUNTIME_DIR` if set, else creates `/run/user/<uid>` (0700),
/// as a login manager would, and exports it for libwayland and clients.
fn runtime_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        // Checked here because libwayland would try 33 socket names and
        // print an error for each before failing.
        let dir = PathBuf::from(dir);
        return match fs::metadata(&dir) {
            Ok(m) if m.is_dir() => Ok(dir),
            Ok(_) => Err(format!(
                "XDG_RUNTIME_DIR={} is not a directory",
                dir.display()
            )),
            Err(e) => Err(format!("XDG_RUNTIME_DIR={}: {e}", dir.display())),
        };
    }
    let uid = fs::metadata("/proc/self")
        .map_err(|e| format!("/proc/self: {e}"))?
        .uid();
    let dir = PathBuf::from(format!("/run/user/{uid}"));
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .map_err(|e| format!("create {}: {e}", dir.display()))?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("chmod {}: {e}", dir.display()))?;
    // Single-threaded at this point; libwayland reads it from the environment.
    std::env::set_var("XDG_RUNTIME_DIR", &dir);
    info!(
        COMPOSITOR,
        "XDG_RUNTIME_DIR={} (created, mode 0700)",
        dir.display()
    );
    Ok(dir)
}

fn run(args: &Args) -> Result<(), String> {
    info!(
        COMPOSITOR,
        "wana-compositor {} starting",
        env!("CARGO_PKG_VERSION")
    );
    let (core, xdg, layer) = wana_wayland::interface_count();
    info!(
        COMPOSITOR,
        "protocol tables: {core} core + {xdg} xdg-shell interfaces + {layer} layer-shell (generated from XML)"
    );
    let (output, drm) = find_output(args)?;
    // The screen (GL context) is created before any client can commit, so
    // textures can be uploaded from protocol callbacks.
    let mut screen = match drm {
        Some(out) => Some(render::Screen::new(out)?),
        None => None,
    };
    let dir = runtime_dir()?;
    let mut display = Display::new(Compositor::new(output.clone(), screen.is_some()))
        .map_err(|e| format!("display: {e}"))?;
    // Input: only with a real display (a headless run is a protocol test).
    let mut libinput = match screen {
        Some(_) => start_input(&mut display, &args.layout)?,
        None => None,
    };
    let (mut advertised, mut privileged) = (Vec::new(), Vec::new());
    for g in globals::globals() {
        let entry = format!("{} v{}", interface_name(g.interface), g.version);
        if g.privileged {
            display.create_privileged_global(g.interface, g.version)?;
            privileged.push(entry);
        } else {
            display.create_global(g.interface, g.version)?;
            advertised.push(entry);
        }
    }
    info!(COMPOSITOR, "globals: {}", advertised.join(", "));
    info!(
        COMPOSITOR,
        "privileged globals (shell only): {}",
        privileged.join(", ")
    );
    info!(
        COMPOSITOR,
        "wl_output {}: {}x{}@{:.2} Hz, {}x{} mm ({})",
        output.name,
        output.width,
        output.height,
        f64::from(output.refresh_mhz) / 1000.0,
        output.mm_width,
        output.mm_height,
        output.description
    );
    let name = display
        .add_socket_auto()
        .map_err(|e| format!("socket in {}: {e}", dir.display()))?;
    let path = display.socket_path().unwrap_or_else(|| dir.join(&name));
    info!(
        COMPOSITOR,
        "listening on {} (WAYLAND_DISPLAY={name})",
        path.display()
    );

    let mut shell = match &args.shell {
        Some(argv) => Some(shell::start(&mut display, argv, &dir)?),
        None => None,
    };
    let mut child = match &args.run {
        Some(argv) => Some(spawn_client(argv, &name, &dir, args.client_debug)?),
        None => None,
    };
    let deadline = args
        .timeout
        .map(|s| Instant::now() + Duration::from_secs(s));
    let start = Instant::now();
    let mut last_present = Instant::now();
    let mut shown_windows = usize::MAX;
    let mut clients = 0u32;
    let result = loop {
        wait_for_work(&display, screen.as_ref(), libinput.as_ref(), TICK)?;
        display
            .dispatch(Duration::ZERO)
            .map_err(|e| format!("event loop: {e}"))?;
        log_events(&mut display, &mut clients);
        if let Some(li) = libinput.as_mut() {
            if readable(li.fd()) {
                read_input(&mut display, li)?;
            }
        }
        // Windows mapped, unmapped or destroyed by the requests above may
        // move the focus.
        display.with_handler(|c, ctx| c.sync_focus(ctx));

        // Presentation: a completed page flip, or the headless clock.
        let mut presented = None;
        if let Some(scr) = screen.as_mut() {
            // Always drain a readable DRM fd, even without a pending flip:
            // an unread event would make poll() return at once forever.
            if readable(scr.drm_fd()) {
                if let Some(us) = scr.handle_drm()? {
                    presented = Some((us / 1000) as u32);
                }
            }
        } else if display.handler().needs_redraw && last_present.elapsed() >= HEADLESS_FRAME {
            display.with_handler(|c, _| c.needs_redraw = false);
            last_present = Instant::now();
            presented = Some(start.elapsed().as_millis() as u32);
        }
        if let Some(ms) = presented {
            display.with_handler(|c, ctx| c.presented(ctx, ms));
        }

        // Redraw when something changed and the previous frame is on screen.
        if let Some(scr) = screen.as_mut() {
            if display.handler().needs_redraw && !scr.flip_pending() {
                let on_screen_now = {
                    let comp = display.handler();
                    let scene = comp.scene();
                    let count = comp.windows.len();
                    let r = scr.draw(scene::BACKGROUND, &scene)?;
                    if count != shown_windows {
                        info!(
                            COMPOSITOR,
                            "frame {}: {count} window(s) on screen",
                            scr.frames()
                        );
                        shown_windows = count;
                    }
                    r
                };
                display.with_handler(|c, ctx| {
                    c.needs_redraw = false;
                    if on_screen_now {
                        c.presented(ctx, start.elapsed().as_millis() as u32);
                    }
                });
            }
        }

        if let Some(s) = shell.as_mut() {
            if let Ok(Some(status)) = s.try_wait() {
                // Restarting the shell comes with the shell itself (step 3).
                if status.success() {
                    info!(COMPOSITOR, "shell exited successfully");
                } else {
                    warn!(COMPOSITOR, "shell exited: {status}");
                }
                shell = None;
                if args.exit_with_shell {
                    // Let its disconnect be processed and logged.
                    display
                        .dispatch(TICK)
                        .map_err(|e| format!("event loop: {e}"))?;
                    log_events(&mut display, &mut clients);
                    break if status.success() {
                        Ok(())
                    } else {
                        Err(format!("shell failed: {status}"))
                    };
                }
            }
        }
        if let Some(c) = child.as_mut() {
            match c.try_wait() {
                Ok(Some(status)) => {
                    // Let the disconnect of that client be processed and logged.
                    display
                        .dispatch(TICK)
                        .map_err(|e| format!("event loop: {e}"))?;
                    log_events(&mut display, &mut clients);
                    let prog = &args.run.as_ref().expect("child implies --run")[0];
                    break if status.success() {
                        info!(COMPOSITOR, "test client {prog} exited successfully");
                        Ok(())
                    } else {
                        Err(format!("test client {prog} failed: {status}"))
                    };
                }
                Ok(None) => {}
                Err(e) => break Err(format!("waitpid: {e}")),
            }
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            if let Some(c) = child.as_mut() {
                let _ = c.kill();
                let _ = c.wait();
                break Err(format!(
                    "timeout after {}s; test client killed",
                    args.timeout.unwrap_or(0)
                ));
            }
            info!(COMPOSITOR, "timeout reached");
            break Ok(());
        }
    };
    if let Some(mut s) = shell.take() {
        let _ = s.kill();
        let _ = s.wait();
        info!(COMPOSITOR, "shell stopped");
    }
    info!(COMPOSITOR, "{clients} client connection(s) served");
    let binds: Vec<String> = {
        let h = display.handler();
        h.globals
            .iter()
            .zip(&h.binds)
            .map(|(g, n)| format!("{} {n}", interface_name(g.interface)))
            .collect()
    };
    info!(COMPOSITOR, "binds: {}", binds.join(", "));
    drop(display);
    drop(libinput);
    drop(screen);
    if path.exists() {
        warn!(
            COMPOSITOR,
            "socket {} still present after shutdown",
            path.display()
        );
    } else {
        info!(COMPOSITOR, "shut down; socket removed");
    }
    result
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}
const POLLIN: i16 = 1;

extern "C" {
    fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
}

/// Waits until the Wayland event loop, the DRM device or libinput has
/// work, or `timeout` passes.
fn wait_for_work(
    display: &Display<Compositor>,
    screen: Option<&render::Screen>,
    libinput: Option<&Libinput>,
    timeout: Duration,
) -> Result<(), String> {
    let mut fds = vec![PollFd {
        fd: display.event_loop_fd(),
        events: POLLIN,
        revents: 0,
    }];
    if let Some(li) = libinput {
        fds.push(PollFd {
            fd: li.fd(),
            events: POLLIN,
            revents: 0,
        });
    }
    let mut timeout = timeout;
    match screen {
        Some(s) => fds.push(PollFd {
            fd: s.drm_fd(),
            events: POLLIN,
            revents: 0,
        }),
        None if display.handler().needs_redraw => timeout = timeout.min(HEADLESS_FRAME),
        None => {}
    }
    // SAFETY: `fds` is a valid array of fds.len() pollfd entries.
    let rc = unsafe {
        poll(
            fds.as_mut_ptr(),
            fds.len() as u64,
            timeout.as_millis() as i32,
        )
    };
    if rc < 0 {
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(format!("poll: {e}"));
        }
    }
    Ok(())
}

/// Compiles the keymap and opens seat0's devices through udev + libinput.
/// Devices present now are reported before any client connects, so the
/// first wl_seat.capabilities is already right.
fn start_input(
    display: &mut Display<Compositor>,
    layout: &str,
) -> Result<Option<Libinput>, String> {
    match Keyboard::new(layout) {
        Ok(kb) => display.with_handler(|c, _| c.set_keyboard(kb))?,
        Err(e) => warn!(Subsystem::Input, "keyboard disabled: {e}"),
    }
    let mut li = match Libinput::new("seat0") {
        Ok(li) => li,
        Err(e) => {
            warn!(Subsystem::Input, "input disabled: {e}");
            return Ok(None);
        }
    };
    read_input(display, &mut li)?;
    Ok(Some(li))
}

/// Reads libinput and routes its events.
fn read_input(display: &mut Display<Compositor>, li: &mut Libinput) -> Result<(), String> {
    li.dispatch()
        .map_err(|e| format!("libinput_dispatch: {e}"))?;
    display.with_handler(|c, ctx| {
        while let Some(ev) = li.next_event() {
            c.input_event(ctx, ev);
        }
    });
    Ok(())
}

/// True if `fd` has data to read now (non-blocking check).
fn readable(fd: i32) -> bool {
    let mut p = PollFd {
        fd,
        events: POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd; zero timeout.
    unsafe { poll(&mut p, 1, 0) > 0 && p.revents & POLLIN != 0 }
}

/// The display clients are told about: from DRM (the first connected
/// connector and its selected mode, as wana-kms/wana-gl use), or a headless
/// description. The DRM device stays open for later steps.
fn find_output(args: &Args) -> Result<(OutputInfo, Option<wana_drm::output::Output>), String> {
    if let Some((w, h, mhz)) = args.headless {
        let info = OutputInfo {
            name: "HEADLESS-1".into(),
            description: "Wana OS headless output".into(),
            make: "Wana".into(),
            model: "headless".into(),
            mm_width: 0,
            mm_height: 0,
            width: w,
            height: h,
            refresh_mhz: mhz,
            preferred: true,
        };
        return Ok((info, None));
    }
    let out = wana_drm::output::find(None)
        .map_err(|e| format!("no display: {e} (use --headless WxH@HZ without one)"))?;
    let driver = out
        .card
        .driver()
        .map(|(d, _)| d)
        .unwrap_or_else(|_| "unknown".into());
    let info = OutputInfo {
        name: out.conn.name.clone(),
        description: format!("{} on {driver}", out.conn.name),
        make: driver.clone(),
        model: out.conn.name.clone(),
        mm_width: out.conn.mm_width as i32,
        mm_height: out.conn.mm_height as i32,
        width: out.mode.width() as i32,
        height: out.mode.height() as i32,
        refresh_mhz: out.mode.refresh_mhz() as i32,
        preferred: out.mode.is_preferred(),
    };
    Ok((info, Some(out)))
}

fn spawn_client(
    argv: &[String],
    display: &str,
    dir: &std::path::Path,
    debug: bool,
) -> Result<Child, String> {
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("XDG_RUNTIME_DIR", dir)
        .env("WAYLAND_DISPLAY", display);
    if debug {
        cmd.env("WAYLAND_DEBUG", "1");
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("cannot run {}: {e}", argv[0]))?;
    info!(
        COMPOSITOR,
        "running test client {} (pid {})",
        argv.join(" "),
        child.id()
    );
    Ok(child)
}

fn log_events(display: &mut Display<Compositor>, clients: &mut u32) {
    for ev in display.take_events() {
        match ev {
            // The shell's connection is a socketpair the compositor made: its
            // credentials would be the compositor's own, so it is named.
            ClientEvent::Connected(c) if c.privileged => {
                *clients += 1;
                info!(
                    COMPOSITOR,
                    "client connected: the shell (private connection)"
                );
            }
            ClientEvent::Connected(c) => {
                *clients += 1;
                info!(
                    COMPOSITOR,
                    "client connected: pid {} uid {} gid {}", c.pid, c.uid, c.gid
                );
            }
            ClientEvent::Disconnected(c) if c.privileged => {
                info!(COMPOSITOR, "client disconnected: the shell")
            }
            ClientEvent::Disconnected(c) => {
                info!(COMPOSITOR, "client disconnected: pid {}", c.pid)
            }
        }
    }
}
