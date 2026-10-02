use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use wana_log::{error, info, warn, Subsystem};

const LOG: Subsystem = Subsystem::Security;
const DESKTOP_UID: u32 = 1000;
const DESKTOP_GID: u32 = 1000;
const SOL_SOCKET: i32 = 1;
const SO_PEERCRED: i32 = 17;

#[repr(C)]
struct PeerCred {
    pid: i32,
    uid: u32,
    gid: u32,
}

unsafe extern "C" {
    fn getsockopt(
        fd: i32,
        level: i32,
        optname: i32,
        optval: *mut std::ffi::c_void,
        optlen: *mut u32,
    ) -> i32;
    fn chown(path: *const std::ffi::c_char, owner: u32, group: u32) -> i32;
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

fn trusted_update_ui(pid: i32, compositor_pid: i32) -> Result<bool, String> {
    if compositor_pid <= 0 || !peer_executable_matches(pid, "/usr/bin/wana-update-ui")? {
        return Ok(false);
    }
    let Some(shell_pid) = parent_pid(pid)? else {
        return Ok(false);
    };
    if !peer_executable_matches(shell_pid, "/usr/bin/wana-shell")? {
        return Ok(false);
    }
    let Some(parent_compositor) = parent_pid(shell_pid)? else {
        return Ok(false);
    };
    if parent_compositor != compositor_pid {
        return Ok(false);
    }
    peer_executable_matches(compositor_pid, "/usr/bin/wana-compositor")
}

fn peer(stream: &UnixStream) -> Result<PeerCred, String> {
    let mut cred = PeerCred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut len = std::mem::size_of::<PeerCred>() as u32;
    // SAFETY: cred is writable storage of len bytes for SO_PEERCRED.
    let rc = unsafe {
        getsockopt(
            stream.as_raw_fd(),
            SOL_SOCKET,
            SO_PEERCRED,
            (&mut cred as *mut PeerCred).cast(),
            &mut len,
        )
    };
    if rc != 0 || len as usize != std::mem::size_of::<PeerCred>() {
        return Err(format!("SO_PEERCRED: {}", std::io::Error::last_os_error()));
    }
    Ok(cred)
}

fn prepare_socket() -> Result<UnixListener, String> {
    fs::create_dir_all("/run/wana").map_err(|e| format!("create /run/wana: {e}"))?;
    let path = Path::new(wana_update::BROKER_SOCKET);
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => {
            fs::remove_file(path).map_err(|e| format!("remove stale {wana_update::BROKER_SOCKET}: {e}"))?;
        }
        Ok(_) => return Err(format!("{wana_update::BROKER_SOCKET}: refusing to replace non-socket path")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{wana_update::BROKER_SOCKET}: {e}")),
    }
    let listener = UnixListener::bind(path).map_err(|e| format!("bind {wana_update::BROKER_SOCKET}: {e}"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o660))
        .map_err(|e| format!("chmod {wana_update::BROKER_SOCKET}: {e}"))?;
    let cpath = std::ffi::CString::new(wana_update::BROKER_SOCKET).map_err(|_| "socket path contains NUL")?;
    // SAFETY: cpath is a live C string.
    if unsafe { chown(cpath.as_ptr(), 0, DESKTOP_GID) } != 0 {
        return Err(format!("chown {wana_update::BROKER_SOCKET}: {}", std::io::Error::last_os_error()));
    }
    Ok(listener)
}

fn respond(stream: &mut UnixStream, value: &str) -> Result<(), String> {
    stream
        .write_all(value.as_bytes())
        .and_then(|_| stream.write_all(b"\n"))
        .map_err(|e| format!("write response: {e}"))
}

fn handle(mut stream: UnixStream) -> Result<(), String> {
    let cred = peer(&stream)?;
    if cred.uid != 0 && cred.uid != DESKTOP_UID {
        warn!(
            LOG,
            "update request rejected: pid={} uid={} gid={}",
            cred.pid,
            cred.uid,
            cred.gid
        );
        return respond(&mut stream, "ERR unauthorized");
    }

    let reader = stream.try_clone().map_err(|e| format!("clone stream: {e}"))?;
    let mut line = String::new();
    BufReader::new(reader)
        .take(129)
        .read_line(&mut line)
        .map_err(|e| format!("read request: {e}"))?;
    if line.len() > 128 {
        return respond(&mut stream, "ERR request-too-long");
    }
    let mut fields = line.split_whitespace();
    let Some(command) = fields.next() else {
        return respond(&mut stream, "ERR empty-request");
    };
    let compositor_pid = match fields.next() {
        Some(value) => match value.parse::<i32>() {
            Ok(pid) if pid > 0 => Some(pid),
            _ => return respond(&mut stream, "ERR invalid-compositor-pid"),
        },
        None => None,
    };
    if fields.next().is_some() {
        return respond(&mut stream, "ERR too-many-fields");
    }

    match command {
        "status" => {
            if let Some(meta) = wana_update::pending_metadata()? {
                respond(
                    &mut stream,
                    &format!("PENDING version={} commit={}", meta.version, meta.commit),
                )
            } else {
                respond(&mut stream, "NONE")
            }
        }
        "fetch-stage" => {
            if cred.uid != 0 && !trusted_update_ui(cred.pid, compositor_pid.unwrap_or(0))? {
                warn!(
                    LOG,
                    "update fetch rejected for untrusted client: pid={} uid={}",
                    cred.pid,
                    cred.uid
                );
                return respond(&mut stream, "ERR unauthorized-client");
            }
            info!(LOG, "update fetch requested by uid={} pid={}", cred.uid, cred.pid);
            match wana_update::fetch_latest_and_stage()? {
                wana_update::FetchOutcome::Current(meta) => respond(
                    &mut stream,
                    &format!("CURRENT version={} commit={}", meta.version, meta.commit),
                ),
                wana_update::FetchOutcome::Staged(meta) => respond(
                    &mut stream,
                    &format!("STAGED version={} commit={}", meta.version, meta.commit),
                ),
            }
        }
        "clear" => {
            if cred.uid != 0 && !trusted_update_ui(cred.pid, compositor_pid.unwrap_or(0))? {
                warn!(
                    LOG,
                    "update clear rejected for untrusted client: pid={} uid={}",
                    cred.pid,
                    cred.uid
                );
                return respond(&mut stream, "ERR unauthorized-client");
            }
            wana_update::clear_pending()?;
            info!(LOG, "pending update cleared by uid={} pid={}", cred.uid, cred.pid);
            respond(&mut stream, "CLEARED")
        }
        _ => respond(&mut stream, "ERR expected status|fetch-stage|clear"),
    }
}

fn run() -> Result<(), String> {
    let listener = prepare_socket()?;
    info!(
        LOG,
        "update broker ready: socket={} mode=0660 owner=0 group={DESKTOP_GID}",
        wana_update::BROKER_SOCKET
    );
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                if let Err(e) = handle(stream) {
                    warn!(LOG, "update request failed: {e}");
                }
            }
            Err(e) => warn!(LOG, "update accept failed: {e}"),
        }
    }
    Err("update listener ended unexpectedly".into())
}

fn main() {
    wana_log::init_from_env();
    if let Err(e) = run() {
        error!(LOG, "update broker: {e}");
        std::process::exit(1);
    }
}
