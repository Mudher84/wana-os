//! wana-compositor: the Wana OS Wayland compositor.
//!
//! Phase 10 step 1 scope: the protocol layer comes up. It prepares
//! `$XDG_RUNTIME_DIR`, opens the Wayland socket through libwayland-server,
//! runs the event loop and logs every client connect/disconnect with its
//! credentials. No globals are advertised yet (step 2).
//!
//! `--run PROGRAM [ARGS...]` starts one client with `WAYLAND_DISPLAY` set,
//! serves it until it exits, then shuts down and returns its result, which
//! makes the compositor usable as a boot test (`wana.run=`).
//! `--client-debug` sets `WAYLAND_DEBUG=1` for that client so the console
//! shows the protocol messages it exchanges.
//!
//! Usage: wana-compositor [--timeout SECONDS] [--client-debug] [--run PROGRAM [ARGS...]]

use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::PathBuf;
use std::process::{Child, Command, ExitCode};
use std::time::{Duration, Instant};
use wana_log::{error, info, warn, Subsystem};
use wana_wayland::server::{ClientEvent, Display};

const COMPOSITOR: Subsystem = Subsystem::Compositor;
const TICK: Duration = Duration::from_millis(100);

#[derive(Default)]
struct Args {
    timeout: Option<u64>,
    client_debug: bool,
    run: Option<Vec<String>>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--timeout" => {
                let v = it.next().ok_or("--timeout needs a value")?;
                a.timeout = Some(v.parse().map_err(|e| format!("--timeout: {e}"))?);
            }
            "--client-debug" => a.client_debug = true,
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
    let (core, xdg) = wana_wayland::interface_count();
    info!(
        COMPOSITOR,
        "protocol tables: {core} core + {xdg} xdg-shell interfaces (generated from XML)"
    );
    let dir = runtime_dir()?;
    let mut display = Display::new().map_err(|e| format!("display: {e}"))?;
    let name = display
        .add_socket_auto()
        .map_err(|e| format!("socket in {}: {e}", dir.display()))?;
    let path = display.socket_path().unwrap_or_else(|| dir.join(&name));
    info!(
        COMPOSITOR,
        "listening on {} (WAYLAND_DISPLAY={name})",
        path.display()
    );

    let mut child = match &args.run {
        Some(argv) => Some(spawn_client(argv, &name, &dir, args.client_debug)?),
        None => None,
    };
    let deadline = args
        .timeout
        .map(|s| Instant::now() + Duration::from_secs(s));
    let mut clients = 0u32;
    let result = loop {
        display
            .dispatch(TICK)
            .map_err(|e| format!("event loop: {e}"))?;
        log_events(&mut display, &mut clients);
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
    info!(COMPOSITOR, "{clients} client connection(s) served");
    drop(display);
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

fn log_events(display: &mut Display, clients: &mut u32) {
    for ev in display.take_events() {
        match ev {
            ClientEvent::Connected(c) => {
                *clients += 1;
                info!(
                    COMPOSITOR,
                    "client connected: pid {} uid {} gid {}", c.pid, c.uid, c.gid
                );
            }
            ClientEvent::Disconnected(c) => {
                info!(COMPOSITOR, "client disconnected: pid {}", c.pid)
            }
        }
    }
}
