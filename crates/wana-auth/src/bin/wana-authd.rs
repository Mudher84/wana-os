use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use wana_auth::{
    crypto::{constant_time_eq, decode_hex, hex, pbkdf2, random_salt, ITERATIONS},
    peer_cred, validate_password, Operation, CREDENTIAL_DIR, CREDENTIAL_FILE, DESKTOP_GID,
    DESKTOP_UID, MAX_PASSWORD_BYTES, SOCKET_PATH,
};
use wana_log::{error, info, warn, Subsystem};

const LOG: Subsystem = Subsystem::Security;

unsafe extern "C" {
    fn chown(path: *const std::ffi::c_char, owner: u32, group: u32) -> i32;
}

fn chown_path(path: &str, owner: u32, group: u32) -> Result<(), String> {
    let cpath = std::ffi::CString::new(path).map_err(|_| "path contains NUL")?;
    // SAFETY: cpath is a live NUL-terminated string for the duration of the call.
    if unsafe { chown(cpath.as_ptr(), owner, group) } == 0 {
        Ok(())
    } else {
        Err(format!("chown {path}: {}", std::io::Error::last_os_error()))
    }
}

fn prepare_state() -> Result<(), String> {
    fs::create_dir_all(CREDENTIAL_DIR).map_err(|e| format!("create {CREDENTIAL_DIR}: {e}"))?;
    let meta = fs::symlink_metadata(CREDENTIAL_DIR)
        .map_err(|e| format!("{CREDENTIAL_DIR}: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() || meta.uid() != 0 {
        return Err(format!("{CREDENTIAL_DIR}: unsafe credential directory"));
    }
    fs::set_permissions(CREDENTIAL_DIR, fs::Permissions::from_mode(0o700))
        .map_err(|e| format!("chmod {CREDENTIAL_DIR}: {e}"))?;
    Ok(())
}

fn credential_exists() -> Result<bool, String> {
    match fs::symlink_metadata(CREDENTIAL_FILE) {
        Ok(meta) => {
            if meta.file_type().is_symlink()
                || !meta.is_file()
                || meta.uid() != 0
                || meta.mode() & 0o077 != 0
                || meta.len() == 0
                || meta.len() > 1024
            {
                return Err(format!("{CREDENTIAL_FILE}: unsafe credential file"));
            }
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{CREDENTIAL_FILE}: {e}")),
    }
}

fn read_hash() -> Result<String, String> {
    if !credential_exists()? {
        return Err("credential is not initialized".into());
    }
    fs::read_to_string(CREDENTIAL_FILE)
        .map(|value| value.trim().to_string())
        .map_err(|e| format!("read {CREDENTIAL_FILE}: {e}"))
}

fn write_hash_atomic(value: &str) -> Result<(), String> {
    prepare_state()?;
    if credential_exists()? {
        return Err("credential already initialized".into());
    }
    let temp = format!("{CREDENTIAL_DIR}/.default.phc.tmp-{}", std::process::id());
    match fs::symlink_metadata(&temp) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            return Err(format!("{temp}: unsafe stale temp"))
        }
        Ok(_) => fs::remove_file(&temp).map_err(|e| format!("remove {temp}: {e}"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{temp}: {e}")),
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)
        .map_err(|e| format!("create {temp}: {e}"))?;
    writeln!(file, "{value}").map_err(|e| format!("write {temp}: {e}"))?;
    file.sync_all().map_err(|e| format!("sync {temp}: {e}"))?;
    fs::rename(&temp, CREDENTIAL_FILE)
        .map_err(|e| format!("rename {temp} -> {CREDENTIAL_FILE}: {e}"))?;
    File::open(CREDENTIAL_DIR)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| format!("sync {CREDENTIAL_DIR}: {e}"))
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = random_salt()?;
    let derived = pbkdf2(password.as_bytes(), &salt, ITERATIONS);
    Ok(format!(
        "WANA-PBKDF2-SHA256${}${}${}",
        ITERATIONS,
        hex(&salt),
        hex(&derived)
    ))
}

fn verify_password(password: &str) -> Result<bool, String> {
    let encoded = read_hash()?;
    let mut parts = encoded.split('$');
    if parts.next() != Some("WANA-PBKDF2-SHA256") {
        return Err("stored credential has an unsupported format".into());
    }
    let iterations = parts
        .next()
        .ok_or("stored credential missing iterations")?
        .parse::<u32>()
        .map_err(|_| "stored credential has invalid iterations")?;
    if iterations < 100_000 || iterations > 1_000_000 {
        return Err("stored credential iterations outside policy".into());
    }
    let salt = decode_hex::<16>(parts.next().ok_or("stored credential missing salt")?)?;
    let want = decode_hex::<32>(parts.next().ok_or("stored credential missing hash")?)?;
    if parts.next().is_some() {
        return Err("stored credential has extra fields".into());
    }
    let got = pbkdf2(password.as_bytes(), &salt, iterations);
    Ok(constant_time_eq(&got, &want))
}

fn prepare_socket() -> Result<UnixListener, String> {
    fs::create_dir_all("/run/wana").map_err(|e| format!("create /run/wana: {e}"))?;
    let path = Path::new(SOCKET_PATH);
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => {
            fs::remove_file(path).map_err(|e| format!("remove stale {SOCKET_PATH}: {e}"))?;
        }
        Ok(_) => return Err(format!("{SOCKET_PATH}: refusing to replace non-socket path")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{SOCKET_PATH}: {e}")),
    }
    let listener = UnixListener::bind(path).map_err(|e| format!("bind {SOCKET_PATH}: {e}"))?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o660))
        .map_err(|e| format!("chmod {SOCKET_PATH}: {e}"))?;
    chown_path(SOCKET_PATH, 0, DESKTOP_GID)?;
    Ok(listener)
}

fn reply(stream: &mut UnixStream, text: &str) -> Result<(), String> {
    stream
        .write_all(text.as_bytes())
        .map_err(|e| format!("write auth reply: {e}"))
}

fn handle(mut stream: UnixStream) -> Result<(), String> {
    let cred = peer_cred(&stream).map_err(|e| format!("SO_PEERCRED: {e}"))?;
    if cred.uid != 0 && cred.uid != DESKTOP_UID {
        warn!(
            LOG,
            "auth request rejected: pid={} uid={} gid={}",
            cred.pid,
            cred.uid,
            cred.gid
        );
        return reply(&mut stream, "ERR unauthorized\n");
    }

    let mut header = [0u8; 5];
    stream
        .read_exact(&mut header)
        .map_err(|e| format!("read auth header: {e}"))?;
    let operation = Operation::from_byte(header[0]).ok_or("unknown auth operation")?;
    let length = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if length > MAX_PASSWORD_BYTES {
        return reply(&mut stream, "ERR credential payload too long\n");
    }
    let mut payload = vec![0u8; length];
    stream
        .read_exact(&mut payload)
        .map_err(|e| format!("read auth payload: {e}"))?;
    let password = std::str::from_utf8(&payload).map_err(|_| "credential payload is not UTF-8")?;

    match operation {
        Operation::Status => {
            if length != 0 {
                return reply(&mut stream, "ERR status takes no payload\n");
            }
            if credential_exists()? {
                reply(&mut stream, "OK ready\n")
            } else {
                reply(&mut stream, "OK setup-required\n")
            }
        }
        Operation::Setup => {
            validate_password(password)?;
            if credential_exists()? {
                return reply(&mut stream, "ERR credential already initialized\n");
            }
            let encoded = hash_password(password)?;
            write_hash_atomic(&encoded)?;
            info!(
                LOG,
                "initial desktop credential created by pid={} uid={}",
                cred.pid,
                cred.uid
            );
            reply(&mut stream, "OK setup\n")
        }
        Operation::Verify => {
            if !credential_exists()? {
                return reply(&mut stream, "ERR credential is not initialized\n");
            }
            let ok = verify_password(password)?;
            if ok {
                info!(LOG, "desktop credential verified: pid={} uid={}", cred.pid, cred.uid);
                reply(&mut stream, "OK verified\n")
            } else {
                warn!(LOG, "desktop credential rejected: pid={} uid={}", cred.pid, cred.uid);
                sleep(Duration::from_millis(500));
                reply(&mut stream, "OK invalid\n")
            }
        }
    }
}

fn run() -> Result<(), String> {
    prepare_state()?;
    let listener = prepare_socket()?;
    info!(
        LOG,
        "auth broker ready: socket={} credential={} mode=0600 scheme=PBKDF2-HMAC-SHA256",
        SOCKET_PATH,
        CREDENTIAL_FILE
    );
    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                if let Err(e) = handle(stream) {
                    warn!(LOG, "auth request failed: {e}");
                }
            }
            Err(e) => warn!(LOG, "auth accept failed: {e}"),
        }
    }
    Err("auth listener ended unexpectedly".into())
}

fn main() {
    wana_log::init_from_env();
    if let Err(e) = run() {
        error!(LOG, "auth broker: {e}");
        std::process::exit(1);
    }
}
