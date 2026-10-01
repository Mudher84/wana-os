//! Device manager bring-up: start udevd and replay ("coldplug") the kernel's
//! device events so every device present at boot is initialized by udev
//! (properties such as `ID_INPUT_KEYBOARD`, permissions, `/dev` symlinks).
//!
//! libinput (Phase 9) only opens input devices that udev has initialized, so
//! this must be done before anything that reads input starts.
//!
//! udevd runs as a direct child of PID 1 (not `--daemon`), so init notices
//! when it dies and restarts it.

use std::fs;
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Where udevd lives: eudev on a split-/usr Buildroot system first, then
/// merged-/usr and systemd-udevd layouts (used by host-built test images).
pub const DAEMONS: &[&str] = &[
    "/sbin/udevd",
    "/usr/sbin/udevd",
    "/usr/bin/udevd",
    "/lib/systemd/systemd-udevd",
    "/usr/lib/systemd/systemd-udevd",
];
pub const ADMS: &[&str] = &[
    "/bin/udevadm",
    "/sbin/udevadm",
    "/usr/bin/udevadm",
    "/usr/sbin/udevadm",
];

/// udevd creates this socket once it accepts control commands; events
/// triggered before that could be missed.
pub const CONTROL: &str = "/run/udev/control";
/// udev's device database: one file per initialized device.
pub const DATA: &str = "/run/udev/data";
const HOTPLUG: &str = "/proc/sys/kernel/hotplug";

pub const CONTROL_TIMEOUT: Duration = Duration::from_secs(10);
const SETTLE_TIMEOUT_SECS: u32 = 30;
const PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

/// First existing path among `candidates`.
pub fn find(candidates: &[&'static str]) -> Option<&'static str> {
    candidates.iter().copied().find(|p| Path::new(p).exists())
}

/// Stops the kernel from forking a usermode helper for every uevent: udevd
/// receives them over netlink instead.
pub fn disable_hotplug_helper() -> io::Result<()> {
    if Path::new(HOTPLUG).exists() {
        fs::write(HOTPLUG, "")?;
    }
    Ok(())
}

/// Starts udevd in the foreground as a child of init.
pub fn spawn_daemon(path: &str) -> io::Result<Child> {
    Command::new(path)
        .env_clear()
        .env("PATH", PATH)
        .current_dir("/")
        .stdin(Stdio::null())
        .spawn()
}

/// Waits until udevd's control socket exists. Returns false on timeout.
pub fn wait_for_control(timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if Path::new(CONTROL).exists() {
            return true;
        }
        sleep(Duration::from_millis(20));
    }
    false
}

/// Replays the add events of all existing devices and waits until udevd has
/// processed them.
pub fn coldplug(adm: &str) -> Result<(), String> {
    let settle_timeout = format!("--timeout={SETTLE_TIMEOUT_SECS}");
    let steps: [&[&str]; 3] = [
        &["trigger", "--type=subsystems", "--action=add"],
        &["trigger", "--type=devices", "--action=add"],
        &["settle", &settle_timeout],
    ];
    for args in steps {
        let status = Command::new(adm)
            .args(args)
            .env_clear()
            .env("PATH", PATH)
            .current_dir("/")
            .stdin(Stdio::null())
            .status()
            .map_err(|e| format!("cannot run {adm}: {e}"))?;
        if !status.success() {
            return Err(format!("udevadm {} failed: {status}", args.join(" ")));
        }
    }
    Ok(())
}

/// What the udev database holds after coldplug.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DbCount {
    /// Initialized devices (files in the database).
    pub devices: usize,
    /// Devices udev classified as input (`E:ID_INPUT=1`).
    pub input: usize,
}

/// Counts the entries of a udev database directory.
pub fn count_db(dir: &Path) -> io::Result<DbCount> {
    let mut count = DbCount::default();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        count.devices += 1;
        // Unreadable entries still count as devices, just not as input.
        if let Ok(text) = fs::read_to_string(entry.path()) {
            if is_input_record(&text) {
                count.input += 1;
            }
        }
    }
    Ok(count)
}

/// True for a database record of a device udev tagged as input.
pub fn is_input_record(text: &str) -> bool {
    text.lines().any(|l| l == "E:ID_INPUT=1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_records_are_recognized() {
        assert!(is_input_record(
            "S:input/by-path/x\nE:ID_INPUT=1\nE:ID_INPUT_KEY=1\n"
        ));
        assert!(!is_input_record("E:ID_INPUT_KEY=1\nE:ID_INPUT=10\n"));
        assert!(!is_input_record(""));
    }

    #[test]
    fn database_is_counted() {
        let dir = std::env::temp_dir().join(format!("wana-udev-db-{}", std::process::id()));
        fs::create_dir_all(dir.join("subdir")).unwrap();
        fs::write(dir.join("c13:64"), "E:ID_INPUT=1\nE:ID_INPUT_KEYBOARD=1\n").unwrap();
        fs::write(dir.join("c13:65"), "E:ID_INPUT=1\nE:ID_INPUT_MOUSE=1\n").unwrap();
        fs::write(dir.join("c226:0"), "E:ID_PATH=pci-0000:00:01.0\n").unwrap();
        let count = count_db(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            count,
            DbCount {
                devices: 3,
                input: 2
            }
        );
    }

    #[test]
    fn candidate_paths_are_absolute() {
        for p in DAEMONS.iter().chain(ADMS) {
            assert!(p.starts_with('/'), "{p}");
        }
    }
}
