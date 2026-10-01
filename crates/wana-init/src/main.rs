//! wana-init: PID 1 of Wana OS.
//!
//! Scope: mount the early kernel filesystems, set the system identity, start
//! udevd and coldplug devices (Phase 9), report `[INIT] info: ready`, then
//! supervise: reap every orphaned process, keep udevd and a debug shell on the
//! console alive. Service management (dependencies, per-service users,
//! restart policies) comes in Phase 17.
//!
//! PID 1 must never exit. Every failure here is logged and the boot
//! continues in a degraded state, so the console shows what broke.

mod cmdline;
mod mounts;
mod sys;
mod udev;

use cmdline::TestAction;
use mounts::Outcome;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command};
use std::thread::sleep;
use std::time::{Duration, Instant};
use sys::{Halt, Reaped};
use wana_log::{debug, error, info, warn, Subsystem};

const INIT: Subsystem = Subsystem::Init;
const SHELL: &str = "/bin/sh";
const SHELL_RESTART_DELAY: Duration = Duration::from_secs(1);
const UDEVD_RESTART_DELAY: Duration = Duration::from_secs(1);
const SERVICES: &str = "/usr/sbin/wana-services";
const SERVICES_DIR: &str = "/etc/wana/services.d";
const SERVICES_RESTART_DELAY: Duration = Duration::from_secs(1);
const LIVE_MOUNT: &str = "/run/wana-live";
const LIVE_IMAGE: &str = "/run/wana-live/live/Wana-OS.img";
const DESKTOP_UID: u32 = 1000;
const DESKTOP_GID: u32 = 1000;
const DESKTOP_RUNTIME: &str = "/run/user/1000";

fn main() {
    let pid = std::process::id();
    if pid != 1 {
        error!(INIT, "wana-init must run as PID 1 (running as pid {pid})");
        std::process::exit(1);
    }
    std::panic::set_hook(Box::new(|info| error!(INIT, "panic: {info}")));

    info!(INIT, "wana-init {} starting", env!("CARGO_PKG_VERSION"));

    let mut problems = Vec::new();
    let failed = early_mounts();
    if failed > 0 {
        problems.push(format!("{failed} early mount(s) failed"));
    }
    match mounts::verify_runtime_hardening() {
        Ok(verified) => info!(
            INIT,
            "security mounts: {verified}/{} hardened (nosuid,nodev,noexec)",
            mounts::HARDENED_RUNTIME_TARGETS.len()
        ),
        Err(e) => {
            error!(INIT, "security mounts: {e}");
            problems.push("runtime mount hardening failed".into());
        }
    }

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
    if opts.shell {
        info!(INIT, "debug console shell: enabled by wana.shell=1");
    } else {
        info!(INIT, "debug console shell: disabled");
    }

    set_identity();
    if opts.live {
        info!(INIT, "boot mode: Live ISO");
    }

    let udevd = if opts.udev {
        start_udev(&mut problems)
    } else {
        info!(INIT, "udev: disabled (wana.udev=0)");
        None
    };

    if opts.live {
        match mount_live_media() {
            Ok((source, bytes)) => info!(
                INIT,
                "live media mounted: {source} -> {LIVE_MOUNT}; installer image {LIVE_IMAGE} ({bytes} bytes)"
            ),
            Err(e) => {
                error!(INIT, "live media: {e}");
                problems.push("live media unavailable".into());
            }
        }
    }

    let want_services = opts
        .services
        .unwrap_or(opts.run.is_none() && opts.test.is_none());
    let services = if want_services {
        if prepare_desktop_runtime(&mut problems) {
            start_services(&mut problems)
        } else {
            None
        }
    } else {
        info!(
            INIT,
            "services: disabled for automated/one-shot boot (set wana.services=1 to override)"
        );
        None
    };

    let uptime = read_first_field("/proc/uptime").unwrap_or_else(|| "?".into());
    if problems.is_empty() {
        info!(INIT, "ready ({uptime}s after kernel start)");
    } else {
        error!(
            INIT,
            "degraded: {} ({uptime}s after kernel start)",
            problems.join("; ")
        );
    }

    if let Some(argv) = &opts.run {
        run_once(argv);
    }

    if let Some(action) = opts.test {
        stop(action);
    }
    supervise(opts.shell, udevd, services)
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

/// Starts udevd and coldplugs the devices present at boot. Returns the
/// udevd child to supervise. Failures are added to `problems`; the boot goes
/// on without udev.
fn start_udev(problems: &mut Vec<String>) -> Option<Child> {
    let Some(daemon) = udev::find(udev::DAEMONS) else {
        info!(INIT, "udev: not installed; /dev from devtmpfs only");
        return None;
    };
    let Some(adm) = udev::find(udev::ADMS) else {
        error!(INIT, "udev: {daemon} present but udevadm is missing");
        problems.push("udevadm missing".into());
        return None;
    };
    if let Err(e) = udev::disable_hotplug_helper() {
        warn!(INIT, "udev: cannot disable the kernel hotplug helper: {e}");
    }
    let start = Instant::now();
    let Some(child) = spawn_udevd(daemon) else {
        problems.push("udevd failed to start".into());
        return None;
    };
    if !udev::wait_for_control(udev::CONTROL_TIMEOUT) {
        error!(
            INIT,
            "udev: {} did not appear within {:?}",
            udev::CONTROL,
            udev::CONTROL_TIMEOUT
        );
        problems.push("udevd not responding".into());
        return Some(child);
    }
    if let Err(e) = udev::coldplug(adm) {
        error!(INIT, "udev: coldplug: {e}");
        problems.push("udev coldplug failed".into());
        return Some(child);
    }
    let ms = start.elapsed().as_millis();
    match udev::count_db(Path::new(udev::DATA)) {
        Ok(n) => info!(
            INIT,
            "udev: coldplug done in {ms} ms: {} devices initialized, {} input", n.devices, n.input
        ),
        Err(e) => warn!(
            INIT,
            "udev: coldplug done in {ms} ms; cannot read {}: {e}",
            udev::DATA
        ),
    }
    Some(child)
}

fn mount_live_media() -> Result<(&'static str, u64), String> {
    fs::create_dir_all(LIVE_MOUNT).map_err(|e| format!("create {LIVE_MOUNT}: {e}"))?;

    let mut last_error = None;
    for source in ["/dev/sr0", "/dev/sr1", "/dev/cdrom"] {
        if !Path::new(source).exists() {
            continue;
        }
        match sys::mount_fs(
            source,
            LIVE_MOUNT,
            "iso9660",
            sys::MS_RDONLY | sys::MS_NOSUID | sys::MS_NODEV | sys::MS_NOEXEC,
            "",
        ) {
            Ok(()) => {
                let meta = fs::metadata(LIVE_IMAGE).map_err(|e| {
                    format!("{source} mounted but {LIVE_IMAGE} is unavailable: {e}")
                })?;
                if !meta.is_file() || meta.len() == 0 {
                    return Err(format!(
                        "{source} mounted but {LIVE_IMAGE} is not a non-empty regular file"
                    ));
                }
                return Ok((source, meta.len()));
            }
            Err(e) => last_error = Some(format!("mount {source} on {LIVE_MOUNT}: {e}")),
        }
    }

    Err(last_error.unwrap_or_else(|| "no ISO9660 live-media device found".into()))
}

fn spawn_udevd(daemon: &str) -> Option<Child> {
    match udev::spawn_daemon(daemon) {
        Ok(child) => {
            info!(INIT, "udev: {daemon} started (pid {})", child.id());
            Some(child)
        }
        Err(e) => {
            error!(INIT, "udev: spawn {daemon}: {e}");
            None
        }
    }
}

fn prepare_desktop_runtime(problems: &mut Vec<String>) -> bool {
    if let Err(e) = fs::create_dir_all(DESKTOP_RUNTIME) {
        error!(INIT, "desktop runtime: create {DESKTOP_RUNTIME}: {e}");
        problems.push("desktop runtime unavailable".into());
        return false;
    }
    if let Err(e) = fs::set_permissions(DESKTOP_RUNTIME, fs::Permissions::from_mode(0o700)) {
        error!(INIT, "desktop runtime: chmod {DESKTOP_RUNTIME}: {e}");
        problems.push("desktop runtime permissions failed".into());
        return false;
    }
    if let Err(e) = sys::chown_path(DESKTOP_RUNTIME, DESKTOP_UID, DESKTOP_GID) {
        error!(INIT, "desktop runtime: chown {DESKTOP_RUNTIME}: {e}");
        problems.push("desktop runtime ownership failed".into());
        return false;
    }
    info!(
        INIT,
        "desktop runtime: {DESKTOP_RUNTIME} uid={DESKTOP_UID} gid={DESKTOP_GID} mode=0700"
    );
    true
}

fn start_services(problems: &mut Vec<String>) -> Option<Child> {
    if !Path::new(SERVICES).is_file() {
        error!(INIT, "service manager missing: {SERVICES}");
        problems.push("service manager missing".into());
        return None;
    }
    if let Err(e) = fs::create_dir_all(SERVICES_DIR) {
        error!(INIT, "services: create {SERVICES_DIR}: {e}");
        problems.push("service directory unavailable".into());
        return None;
    }
    match spawn_services() {
        Some(child) => Some(child),
        None => {
            problems.push("service manager failed to start".into());
            None
        }
    }
}

fn spawn_services() -> Option<Child> {
    match Command::new(SERVICES)
        .args(["run", SERVICES_DIR])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .current_dir("/")
        .spawn()
    {
        Ok(child) => {
            info!(INIT, "services: manager started (pid {})", child.id());
            Some(child)
        }
        Err(e) => {
            error!(INIT, "services: spawn {SERVICES}: {e}");
            None
        }
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

/// Reaps children forever and keeps udevd and the console shell alive.
fn supervise(want_shell: bool, mut udevd: Option<Child>, mut services: Option<Child>) -> ! {
    let mut shell = if want_shell { spawn_shell() } else { None };
    loop {
        match sys::reap_any(true) {
            Ok(Reaped::Child { pid, status }) => {
                if udevd.as_ref().map(Child::id) == Some(pid as u32) {
                    error!(
                        INIT,
                        "udev: udevd exited (wait status {status}), restarting"
                    );
                    sleep(UDEVD_RESTART_DELAY);
                    udevd = udev::find(udev::DAEMONS).and_then(spawn_udevd);
                } else if services.as_ref().map(Child::id) == Some(pid as u32) {
                    error!(
                        INIT,
                        "services: manager exited (wait status {status}), restarting"
                    );
                    sleep(SERVICES_RESTART_DELAY);
                    services = spawn_services();
                } else if shell.as_ref().map(Child::id) == Some(pid as u32) {
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
