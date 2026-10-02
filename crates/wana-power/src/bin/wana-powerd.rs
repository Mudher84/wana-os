use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use wana_log::{error, info, warn, Subsystem};
use wana_power::{authorized, peer_cred, Command, DESKTOP_GID, SOCKET_PATH};

const LOG: Subsystem = Subsystem::Security;
const RB_POWER_OFF: i32 = 0x4321_fedc_u32 as i32;
const RB_AUTOBOOT: i32 = 0x0123_4567;

unsafe extern "C" {
    fn chown(path: *const std::ffi::c_char, owner: u32, group: u32) -> i32;
    fn reboot(cmd: i32) -> i32;
    fn sync();
}

fn peer_executable_matches(pid: i32, expected: &str) -> Result<bool, String> {
    if pid <= 0 {
        return Ok(false);
    }
    let trusted = fs::metadata(expected).map_err(|e| format!("metadata {expected}: {e}"))?;
    if !trusted.is_file() || trusted.uid() != 0 || trusted.mode() & 0o022 != 0 {
        return Err(format!("{expected}: trusted client executable is not root-owned/read-only"));
    }
    let proc_exe = format!("/proc/{pid}/exe");
    let peer = match fs::metadata(&proc_exe) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(format!("metadata {proc_exe}: {e}")),
    };
    Ok(peer.dev() == trusted.dev() && peer.ino() == trusted.ino())
}

fn parent_pid(pid: i32) -> Result<Option<i32>, String> {
    if pid <= 0 {
        return Ok(None);
    }
    let path = format!("/proc/{pid}/status");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("read {path}: {e}")),
    };
    let ppid = text
        .lines()
        .find_map(|line| line.strip_prefix("PPid:"))
        .and_then(|value| value.trim().parse::<i32>().ok())
        .ok_or_else(|| format!("{path}: PPid missing or invalid"))?;
    Ok((ppid > 0).then_some(ppid))
}

fn trusted_shell_ui(pid: i32, ui: &str) -> Result<bool, String> {
    if !peer_executable_matches(pid, ui)? {
        return Ok(false);
    }
    let Some(parent) = parent_pid(pid)? else {
        return Ok(false);
    };
    peer_executable_matches(parent, "/usr/bin/wana-shell")
}

fn chown_socket(path: &str) -> Result<(), String> {
    let path = std::ffi::CString::new(path).map_err(|_| "socket path contains NUL")?;
    // SAFETY: path is a live C string and chown only reads it.
    if unsafe { chown(path.as_ptr(), 0, DESKTOP_GID) } == 0 {
        Ok(())
    } else {
        Err(format!("chown {SOCKET_PATH}: {}", std::io::Error::last_os_error()))
    }
}

fn prepare_socket() -> Result<UnixListener, String> {
    let dir = Path::new("/run/wana");
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("chmod {}: {e}", dir.display()))?;

    let socket = Path::new(SOCKET_PATH);
    match fs::symlink_metadata(socket) {
        Ok(meta) if meta.file_type().is_socket() => {
            fs::remove_file(socket).map_err(|e| format!("remove stale {SOCKET_PATH}: {e}"))?
        }
        Ok(_) => return Err(format!("{SOCKET_PATH}: refusing to replace non-socket path")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{SOCKET_PATH}: {e}")),
    }

    let listener = UnixListener::bind(socket).map_err(|e| format!("bind {SOCKET_PATH}: {e}"))?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o660))
        .map_err(|e| format!("chmod {SOCKET_PATH}: {e}"))?;
    chown_socket(SOCKET_PATH)?;
    Ok(listener)
}

fn halt(command: Command) -> ! {
    // SAFETY: sync takes no arguments and only flushes dirty filesystem state.
    unsafe { sync() };
    let code = match command {
        Command::PowerOff => RB_POWER_OFF,
        Command::Reboot => RB_AUTOBOOT,
        Command::Status => unreachable!("status never halts"),
    };
    // SAFETY: daemon runs as root and code is one of Linux's reboot command constants.
    unsafe { reboot(code) };
    let err = std::io::Error::last_os_error();
    error!(LOG, "power {} failed: {err}", command.as_str());
    std::process::exit(1);
}

fn handle(mut stream: UnixStream) -> Result<(), String> {
    let cred = peer_cred(&stream).map_err(|e| format!("SO_PEERCRED: {e}"))?;
    if !authorized(cred.uid) {
        warn!(
            LOG,
            "power request rejected: pid={} uid={} gid={}",
            cred.pid,
            cred.uid,
            cred.gid
        );
        stream.write_all(b"ERR unauthorized\n").map_err(|e| e.to_string())?;
        return Ok(());
    }

    let read_stream = stream.try_clone().map_err(|e| format!("clone stream: {e}"))?;
    let mut line = String::new();
    BufReader::new(read_stream)
        .take(65)
        .read_line(&mut line)
        .map_err(|e| format!("read request: {e}"))?;
    if line.len() > 64 {
        stream.write_all(b"ERR request-too-long\n").map_err(|e| e.to_string())?;
        return Ok(());
    }
    let command = match Command::parse(&line) {
        Ok(command) => command,
        Err(reason) => {
            stream
                .write_all(format!("ERR {reason}\n").as_bytes())
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
    };

    match command {
        Command::Status => {
            stream.write_all(b"OK ready\n").map_err(|e| e.to_string())?;
            info!(LOG, "power status: pid={} uid={} authorized", cred.pid, cred.uid);
            Ok(())
        }
        Command::PowerOff | Command::Reboot => {
            let trusted_ui = trusted_shell_ui(cred.pid, "/usr/bin/wana-power-ui")?
                || trusted_shell_ui(cred.pid, "/usr/bin/wana-update-ui")?;
            if cred.uid != 0 && !trusted_ui {
                warn!(
                    LOG,
                    "power action rejected for untrusted client: {} pid={} uid={}",
                    command.as_str(),
                    cred.pid,
                    cred.uid
                );
                stream
                    .write_all(b"ERR unauthorized-client\n")
                    .map_err(|e| e.to_string())?;
                return Ok(());
            }
            stream
                .write_all(format!("OK {}\n", command.as_str()).as_bytes())
                .map_err(|e| e.to_string())?;
            let _ = stream.flush();
            info!(
                LOG,
                "power action authorized: {} pid={} uid={}",
                command.as_str(),
                cred.pid,
                cred.uid
            );
            halt(command)
        }
    }
}

fn run() -> Result<(), String> {
    let listener = prepare_socket()?;
    info!(
        LOG,
        "power broker ready: socket={} mode=0660 owner=0 group={DESKTOP_GID}",
        SOCKET_PATH
    );
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                if let Err(e) = handle(stream) {
                    warn!(LOG, "power request failed: {e}");
                }
            }
            Err(e) => warn!(LOG, "power accept failed: {e}"),
        }
    }
    Err("power listener ended unexpectedly".into())
}

fn main() {
    wana_log::init_from_env();
    if let Err(e) = run() {
        error!(LOG, "power broker: {e}");
        std::process::exit(1);
    }
}
