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
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
const DATA_MOUNT: &str = "/data";
const DATA_DEVICE: &str = "/dev/disk/by-label/WANA-DATA";
const UPDATE_DATA_MOUNT: &str = "/run/wana-update-data";
const UPDATE_NEW_ROOT: &str = "/run/wana-update-new";
const UPDATE_ESP_MOUNT: &str = "/run/wana-update-esp";
const UPDATE_ESP_DEVICE: &str = "/dev/disk/by-label/WANA-ESP";

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
    if opts.update {
        info!(INIT, "boot mode: isolated A/B update environment; active slot={}", opts.active_slot);
    } else if opts.live {
        info!(INIT, "boot mode: Live ISO");
    } else {
        match sys::remount_rw("/") {
            Ok(()) => info!(INIT, "installed root: remounted read-write"),
            Err(e) => {
                error!(INIT, "installed root: remount read-write failed: {e}");
                problems.push("installed root is not writable".into());
            }
        }
    }

    let udevd = if opts.udev {
        start_udev(&mut problems)
    } else {
        info!(INIT, "udev: disabled (wana.udev=0)");
        None
    };

    if opts.update {
        match apply_staged_update(&opts) {
            Ok(slot) => {
                info!(INIT, "update applied successfully; next slot={slot}; rebooting");
                let err = sys::halt(Halt::Reboot);
                error!(INIT, "update reboot failed: {err}");
                supervise(false, udevd, None);
            }
            Err(e) => {
                error!(INIT, "update failed: {e}");
                if let Err(mark_error) = quarantine_failed_update(&opts, &e) {
                    error!(INIT, "cannot quarantine failed update: {mark_error}");
                }
                let err = sys::halt(Halt::Reboot);
                error!(INIT, "failed-update reboot failed: {err}");
                supervise(false, udevd, None);
            }
        }
    }

    if !opts.live && !opts.update {
        prepare_persistent_data(&mut problems);
    }

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

fn device_from_spec(spec: &str) -> Result<PathBuf, String> {
    let (kind, id) = spec
        .split_once('=')
        .ok_or_else(|| format!("invalid device spec {spec:?}"))?;
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F' | b'-'))
    {
        return Err(format!("invalid device identifier {id:?}"));
    }
    match kind {
        "PARTUUID" => Ok(Path::new("/dev/disk/by-partuuid").join(id)),
        "UUID" => Ok(Path::new("/dev/disk/by-uuid").join(id)),
        _ => Err(format!("unsupported device spec {spec:?}")),
    }
}

fn require_block_device(path: &Path) -> Result<(), String> {
    if !wait_for_path(path, Duration::from_secs(10)) {
        return Err(format!("device {} did not appear", path.display()));
    }
    let meta = fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.file_type().is_block_device() {
        return Err(format!("{} is not a block device", path.display()));
    }
    Ok(())
}

fn mount_update_data(spec: &str) -> Result<PathBuf, String> {
    let device = device_from_spec(spec)?;
    require_block_device(&device)?;
    fs::create_dir_all(UPDATE_DATA_MOUNT)
        .map_err(|e| format!("create {UPDATE_DATA_MOUNT}: {e}"))?;
    sys::mount_fs(
        device.to_str().ok_or("data device path is not UTF-8")?,
        UPDATE_DATA_MOUNT,
        "ext4",
        sys::MS_NOSUID | sys::MS_NODEV,
        "",
    )
    .map_err(|e| format!("mount update data {}: {e}", device.display()))?;
    Ok(device)
}

fn verify_new_slot(device: &Path) -> Result<(), String> {
    fs::create_dir_all(UPDATE_NEW_ROOT)
        .map_err(|e| format!("create {UPDATE_NEW_ROOT}: {e}"))?;
    sys::mount_fs(
        device.to_str().ok_or("root slot device path is not UTF-8")?,
        UPDATE_NEW_ROOT,
        "ext4",
        sys::MS_RDONLY | sys::MS_NOSUID | sys::MS_NODEV,
        "",
    )
    .map_err(|e| format!("mount new root {}: {e}", device.display()))?;

    let result = (|| -> Result<(), String> {
        for relative in ["usr/sbin/wana-init", "usr/bin/wana-session", "etc/hostname"] {
            let path = Path::new(UPDATE_NEW_ROOT).join(relative);
            let meta = fs::symlink_metadata(&path)
                .map_err(|e| format!("new root missing {}: {e}", path.display()))?;
            if meta.file_type().is_symlink() || !meta.is_file() || meta.len() == 0 {
                return Err(format!("new root has invalid {}", path.display()));
            }
        }
        Ok(())
    })();

    let unmount = sys::unmount(UPDATE_NEW_ROOT)
        .map_err(|e| format!("unmount verified root: {e}"));
    result?;
    unmount
}

fn replace_slot_kernel(
    pending: &Path,
    metadata: &wana_update::Metadata,
    slot: char,
) -> Result<(), String> {
    if !wait_for_path(Path::new(UPDATE_ESP_DEVICE), Duration::from_secs(10)) {
        return Err(format!("{UPDATE_ESP_DEVICE} did not appear"));
    }
    fs::create_dir_all(UPDATE_ESP_MOUNT)
        .map_err(|e| format!("create {UPDATE_ESP_MOUNT}: {e}"))?;
    sys::mount_fs(
        UPDATE_ESP_DEVICE,
        UPDATE_ESP_MOUNT,
        "vfat",
        sys::MS_NOSUID | sys::MS_NODEV | sys::MS_NOEXEC,
        "",
    )
    .map_err(|e| format!("mount ESP: {e}"))?;

    let result = (|| -> Result<(), String> {
        let dir = Path::new(UPDATE_ESP_MOUNT).join("wana");
        fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        let final_path = dir.join(format!("bzImage-{slot}"));
        let temp_path = dir.join(format!(".bzImage-{slot}.new"));
        if let Ok(meta) = fs::symlink_metadata(&temp_path) {
            if meta.file_type().is_symlink() || !meta.is_file() {
                return Err(format!("unsafe stale kernel temp {}", temp_path.display()));
            }
            fs::remove_file(&temp_path)
                .map_err(|e| format!("remove {}: {e}", temp_path.display()))?;
        }
        fs::copy(pending.join(wana_update::KERNEL_FILE), &temp_path)
            .map_err(|e| format!("copy new slot kernel: {e}"))?;
        let (size, digest) = wana_update::sha256_file(&temp_path)?;
        if size != metadata.kernel_size || digest != metadata.kernel_sha256 {
            let _ = fs::remove_file(&temp_path);
            return Err("copied slot kernel failed verification".into());
        }
        File::open(&temp_path)
            .and_then(|file| file.sync_all())
            .map_err(|e| format!("sync {}: {e}", temp_path.display()))?;
        fs::rename(&temp_path, &final_path)
            .map_err(|e| format!("replace {}: {e}", final_path.display()))?;
        Ok(())
    })();

    let unmount = sys::unmount(UPDATE_ESP_MOUNT).map_err(|e| format!("unmount ESP: {e}"));
    result?;
    unmount
}

fn write_slot_selector(update_root: &Path, slot: char) -> Result<(), String> {
    let selector = update_root.join("boot-slot.cfg");
    if let Ok(meta) = fs::symlink_metadata(&selector) {
        if meta.file_type().is_symlink() || !meta.is_file() || meta.uid() != 0 {
            return Err(format!("unsafe existing selector {}", selector.display()));
        }
    }
    let temp = update_root.join(format!(".boot-slot.cfg.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|e| format!("create {}: {e}", temp.display()))?;
    writeln!(file, "set wana_slot=\"{slot}\"")
        .map_err(|e| format!("write {}: {e}", temp.display()))?;
    file.sync_all()
        .map_err(|e| format!("sync {}: {e}", temp.display()))?;
    fs::rename(&temp, &selector)
        .map_err(|e| format!("replace {}: {e}", selector.display()))?;
    File::open(update_root)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| format!("sync {}: {e}", update_root.display()))
}

fn finish_update_state(pending: &Path, update_root: &Path) -> Result<(), String> {
    let applied = update_root.join("last-applied.txt");
    let temp = update_root.join(format!(".last-applied.tmp-{}", std::process::id()));
    if let Ok(meta) = fs::symlink_metadata(&temp) {
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(format!("unsafe stale applied temp {}", temp.display()));
        }
        fs::remove_file(&temp).map_err(|e| format!("remove {}: {e}", temp.display()))?;
    }
    fs::copy(pending.join(wana_update::UPDATE_FILE), &temp)
        .map_err(|e| format!("copy applied metadata: {e}"))?;
    File::open(&temp)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("sync {}: {e}", temp.display()))?;
    fs::rename(&temp, &applied)
        .map_err(|e| format!("replace {}: {e}", applied.display()))?;

    let meta = fs::symlink_metadata(pending)
        .map_err(|e| format!("{}: {e}", pending.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() || meta.uid() != 0 {
        return Err(format!("unsafe pending directory {}", pending.display()));
    }
    fs::remove_dir_all(pending)
        .map_err(|e| format!("remove completed pending update: {e}"))?;
    File::open(update_root)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| format!("sync {}: {e}", update_root.display()))
}

fn apply_staged_update(opts: &cmdline::Options) -> Result<char, String> {
    let data_spec = opts.data.as_deref().ok_or("update boot missing wana.data")?;
    let root_a = opts.root_a.as_deref().ok_or("update boot missing wana.root_a")?;
    let root_b = opts.root_b.as_deref().ok_or("update boot missing wana.root_b")?;
    let inactive = if opts.active_slot == 'A' { 'B' } else { 'A' };
    let target_spec = if inactive == 'A' { root_a } else { root_b };

    let _data_device = mount_update_data(data_spec)?;
    let update_root = Path::new(UPDATE_DATA_MOUNT).join("var/lib/wana/update");
    let pending = update_root.join("pending");
    let pending_meta = fs::symlink_metadata(&pending)
        .map_err(|e| format!("{}: {e}", pending.display()))?;
    if pending_meta.file_type().is_symlink() || !pending_meta.is_dir() || pending_meta.uid() != 0 {
        return Err(format!("{}: unsafe pending directory", pending.display()));
    }

    let metadata = wana_update::verify_staged(&pending)?;
    info!(
        INIT,
        "update verified: version={} commit={} active={} target={inactive}",
        metadata.version,
        metadata.commit,
        opts.active_slot
    );

    let compressed = pending.join(wana_update::ROOTFS_FILE);
    let zstd_test = Command::new("/usr/bin/zstd")
        .args(["-q", "-t"])
        .arg(&compressed)
        .status()
        .map_err(|e| format!("run zstd integrity test: {e}"))?;
    if !zstd_test.success() {
        return Err(format!("zstd integrity test failed: {zstd_test}"));
    }

    let target = device_from_spec(target_spec)?;
    require_block_device(&target)?;
    let output = OpenOptions::new()
        .write(true)
        .open(&target)
        .map_err(|e| format!("open inactive root {}: {e}", target.display()))?;
    let sync_handle = output
        .try_clone()
        .map_err(|e| format!("clone inactive root handle: {e}"))?;
    let status = Command::new("/usr/bin/zstd")
        .args(["-d", "-q", "-c"])
        .arg(&compressed)
        .stdout(Stdio::from(output))
        .status()
        .map_err(|e| format!("decompress root update: {e}"))?;
    if !status.success() {
        return Err(format!("root update decompression failed: {status}"));
    }
    sync_handle
        .sync_all()
        .map_err(|e| format!("sync inactive root {}: {e}", target.display()))?;
    info!(INIT, "update root written: slot={inactive} device={}", target.display());

    verify_new_slot(&target)?;
    info!(INIT, "update root verified: slot={inactive}");

    replace_slot_kernel(&pending, &metadata, inactive)?;
    info!(INIT, "update kernel installed: slot={inactive}");

    // Pending is removed before the selector changes. If power is lost here,
    // GRUB still boots the old active slot rather than re-entering update mode.
    finish_update_state(&pending, &update_root)?;
    write_slot_selector(&update_root, inactive)?;
    info!(INIT, "update selector committed: slot={inactive}");
    Ok(inactive)
}

fn quarantine_failed_update(opts: &cmdline::Options, reason: &str) -> Result<(), String> {
    let data_spec = opts.data.as_deref().ok_or("failed update has no wana.data")?;
    let pending_root = Path::new(UPDATE_DATA_MOUNT).join("var/lib/wana/update/pending");
    if !pending_root.exists() {
        let _ = mount_update_data(data_spec)?;
    }
    let marker = pending_root.join(wana_update::UPDATE_FILE);
    if marker.exists() {
        let failed = pending_root.join("update.failed");
        if failed.exists() {
            fs::remove_file(&failed)
                .map_err(|e| format!("remove old {}: {e}", failed.display()))?;
        }
        fs::rename(&marker, &failed)
            .map_err(|e| format!("quarantine {}: {e}", marker.display()))?;
        fs::write(pending_root.join("update-error.txt"), format!("{reason}\n"))
            .map_err(|e| format!("record update failure: {e}"))?;
        info!(INIT, "failed update quarantined; normal active slot will boot");
    }
    Ok(())
}

fn wait_for_path(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        sleep(Duration::from_millis(50));
    }
    path.exists()
}

fn ensure_owned_dir(path: &str, mode: u32, uid: u32, gid: u32) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("create {path}: {e}"))?;
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{path}: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!("{path}: expected real directory"));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|e| format!("chmod {path}: {e}"))?;
    sys::chown_path(path, uid, gid).map_err(|e| format!("chown {path}: {e}"))
}

fn seed_wifi_config() -> Result<(), String> {
    let source = Path::new("/var/lib/wana/wifi.conf");
    let target = Path::new("/data/var/lib/wana/wifi.conf");
    if target.exists() || !source.is_file() {
        return Ok(());
    }
    fs::copy(source, target)
        .map_err(|e| format!("seed {} -> {}: {e}", source.display(), target.display()))?;
    fs::set_permissions(target, fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod {}: {e}", target.display()))
}

fn prepare_persistent_data(problems: &mut Vec<String>) -> bool {
    if !wait_for_path(Path::new(DATA_DEVICE), Duration::from_secs(5)) {
        warn!(INIT, "persistent data: {DATA_DEVICE} not found; using root filesystem fallback");
        problems.push("persistent data partition unavailable".into());
        return false;
    }
    if let Err(e) = fs::create_dir_all(DATA_MOUNT) {
        error!(INIT, "persistent data: create {DATA_MOUNT}: {e}");
        problems.push("persistent data mountpoint unavailable".into());
        return false;
    }
    if let Err(e) = sys::mount_fs(
        DATA_DEVICE,
        DATA_MOUNT,
        "ext4",
        sys::MS_NOSUID | sys::MS_NODEV,
        "",
    ) {
        error!(INIT, "persistent data: mount {DATA_DEVICE} on {DATA_MOUNT}: {e}");
        problems.push("persistent data mount failed".into());
        return false;
    }

    let setup = (|| -> Result<(), String> {
        ensure_owned_dir("/data/home", 0o755, 0, 0)?;
        ensure_owned_dir("/data/home/wana", 0o700, DESKTOP_UID, DESKTOP_GID)?;
        ensure_owned_dir("/data/var", 0o755, 0, 0)?;
        ensure_owned_dir("/data/var/lib", 0o755, 0, 0)?;
        ensure_owned_dir("/data/var/lib/wana", 0o755, 0, 0)?;
        ensure_owned_dir("/data/var/lib/waydroid", 0o700, 0, 0)?;
        ensure_owned_dir("/data/var/lib/bluetooth", 0o700, 0, 0)?;
        seed_wifi_config()?;

        for (source, target) in [
            ("/data/home/wana", "/home/wana"),
            ("/data/var/lib/wana", "/var/lib/wana"),
            ("/data/var/lib/waydroid", "/var/lib/waydroid"),
            ("/data/var/lib/bluetooth", "/var/lib/bluetooth"),
        ] {
            fs::create_dir_all(target).map_err(|e| format!("create {target}: {e}"))?;
            sys::bind_mount(source, target)
                .map_err(|e| format!("bind {source} -> {target}: {e}"))?;
        }
        Ok(())
    })();

    match setup {
        Ok(()) => {
            info!(
                INIT,
                "persistent data: {DATA_DEVICE} -> {DATA_MOUNT}; home,wifi,updates,waydroid,bluetooth bound"
            );
            true
        }
        Err(e) => {
            error!(INIT, "persistent data: {e}");
            problems.push("persistent data setup failed".into());
            false
        }
    }
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
    if let Err(e) = fs::create_dir_all("/run/dbus") {
        error!(INIT, "runtime: create /run/dbus: {e}");
        problems.push("system bus runtime unavailable".into());
        return false;
    }
    if let Err(e) = fs::set_permissions("/run/dbus", fs::Permissions::from_mode(0o755)) {
        error!(INIT, "runtime: chmod /run/dbus: {e}");
        problems.push("system bus runtime permissions failed".into());
        return false;
    }
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
