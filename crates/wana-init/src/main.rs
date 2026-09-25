//! wana-init: PID 1 of Wana OS.
//!
//! Phase 4 scope: mount the early kernel filesystems, set the system
//! identity, report `[INIT] info: ready`, then supervise: reap every orphaned
//! process and keep a debug shell on the console. Service management
//! (dependencies, per-service users, restart policies) comes in Phase 17.
//!
//! PID 1 must never exit. Every failure here is logged and the boot
//! continues in a degraded state, so the console shows what broke.

mod cmdline;
mod mounts;
mod sys;

use cmdline::TestAction;
use mounts::Outcome;
use std::fs;
use std::path::Path;
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::Duration;
use sys::{Halt, Reaped};
use wana_log::{debug, error, info, warn, Subsystem};

const INIT: Subsystem = Subsystem::Init;
const SHELL: &str = "/bin/sh";
const SHELL_RESTART_DELAY: Duration = Duration::from_secs(1);

fn main() {
    let pid = std::process::id();
    if pid != 1 {
        error!(INIT, "wana-init must run as PID 1 (running as pid {pid})");
        std::process::exit(1);
    }
    std::panic::set_hook(Box::new(|info| error!(INIT, "panic: {info}")));

    info!(INIT, "wana-init {} starting", env!("CARGO_PKG_VERSION"));

    let failed = early_mounts();

    let opts = match fs::read_to_string("/proc/cmdline") {
        Ok(line) => cmdline::parse(&line),
        Err(e) => {
            warn!(INIT, "cannot read /proc/cmdline: {e}; using defaults");
            cmdline::Options::default()
        }
    };
    wana_log::set_max_level(opts.log_level);
    for w in &opts.warnings {
        warn!(INIT, "{w}");
    }

    set_identity();

    let uptime = read_first_field("/proc/uptime").unwrap_or_else(|| "?".into());
    if failed == 0 {
        info!(INIT, "ready ({uptime}s after kernel start)");
    } else {
        error!(
            INIT,
            "degraded: {failed} early mount(s) failed ({uptime}s after kernel start)"
        );
    }

    if let Some(argv) = &opts.run {
        run_once(argv);
    }

    if let Some(action) = opts.test {
        stop(action);
    }
    supervise(opts.shell)
}

/// Performs the early mounts. Returns the number of failures.
fn early_mounts() -> usize {
    let mut failed = 0;
    for m in mounts::EARLY {
        match mounts::apply(m) {
            Outcome::Mounted => debug!(INIT, "mounted {} on {}", m.fstype, m.target),
            Outcome::AlreadyMounted => debug!(INIT, "{} already mounted", m.target),
            Outcome::Failed(e) => {
                error!(INIT, "mount {} on {}: {e}", m.fstype, m.target);
                failed += 1;
            }
        }
    }
    info!(
        INIT,
        "early mounts: {} ok, {failed} failed",
        mounts::EARLY.len() - failed
    );
    failed
}

/// Sets the hostname from /etc/hostname and logs the kernel release.
fn set_identity() {
    match read_first_field("/etc/hostname") {
        Some(name) => match sys::set_hostname(&name) {
            Ok(()) => info!(INIT, "hostname: {name}"),
            Err(e) => warn!(INIT, "sethostname({name}): {e}"),
        },
        None => warn!(INIT, "/etc/hostname missing or empty; hostname not set"),
    }
    if let Some(release) = read_first_field("/proc/sys/kernel/osrelease") {
        info!(INIT, "kernel: Linux {release}");
    }
}

/// First whitespace-separated field of a small text file.
fn read_first_field(path: &str) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    text.split_whitespace().next().map(str::to_owned)
}

/// Runs one program to completion (`wana.run=`), for bring-up and tests.
/// Its output goes to the console; its exit status is logged.
fn run_once(argv: &[String]) {
    info!(INIT, "running {}", argv.join(" "));
    match Command::new(&argv[0])
        .args(&argv[1..])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .current_dir("/")
        .status()
    {
        Ok(status) if status.success() => info!(INIT, "{} exited successfully", argv[0]),
        Ok(status) => error!(INIT, "{} failed: {status}", argv[0]),
        Err(e) => error!(INIT, "cannot run {}: {e}", argv[0]),
    }
}

/// Stops the machine for an automated test boot. Returns only on failure.
fn stop(action: TestAction) {
    let how = match action {
        TestAction::PowerOff => Halt::PowerOff,
        TestAction::Reboot => Halt::Reboot,
    };
    info!(INIT, "test boot: {how:?}");
    let err = sys::halt(how);
    error!(INIT, "{how:?} failed: {err}; continuing to supervise");
}

/// Reaps children forever and keeps the console shell alive.
fn supervise(want_shell: bool) -> ! {
    let mut shell = if want_shell { spawn_shell() } else { None };
    loop {
        match sys::reap_any(true) {
            Ok(Reaped::Child { pid, status }) => {
                if shell.as_ref().map(Child::id) == Some(pid as u32) {
                    info!(
                        INIT,
                        "console shell exited (wait status {status}), restarting"
                    );
                    sleep(SHELL_RESTART_DELAY);
                    // waitpid(-1) already reaped it; replacing the handle drops
                    // the old one without waiting again.
                    shell = spawn_shell();
                } else {
                    debug!(INIT, "reaped pid {pid} (wait status {status})");
                }
            }
            // No children right now; orphans may appear later.
            Ok(Reaped::NoChildren) | Ok(Reaped::Nothing) => sleep(Duration::from_secs(5)),
            Err(e) => {
                error!(INIT, "waitpid: {e}");
                sleep(Duration::from_secs(1));
            }
        }
    }
}

fn spawn_shell() -> Option<Child> {
    if !Path::new(SHELL).exists() {
        warn!(INIT, "{SHELL} not present; no console shell");
        return None;
    }
    match Command::new(SHELL)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("HOME", "/root")
        .env("TERM", "linux")
        .current_dir("/")
        .spawn()
    {
        Ok(child) => {
            info!(INIT, "console shell started (pid {})", child.id());
            Some(child)
        }
        Err(e) => {
            error!(INIT, "spawn {SHELL}: {e}");
            None
        }
    }
}
