pub mod crypto;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;

pub const SOCKET_PATH: &str = "/run/wana/auth.sock";
pub const CREDENTIAL_DIR: &str = "/var/lib/wana/auth";
pub const CREDENTIAL_FILE: &str = "/var/lib/wana/auth/default.cred";
pub const DESKTOP_UID: u32 = 1000;
pub const DESKTOP_GID: u32 = 1000;
pub const MAX_PASSWORD_BYTES: usize = 128;
pub const MIN_PASSWORD_BYTES: usize = 8;

const SOL_SOCKET: i32 = 1;
const SO_PEERCRED: i32 = 17;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCred {
    pub pid: i32,
    pub uid: u32,
    pub gid: u32,
}

unsafe extern "C" {
    fn getsockopt(
        fd: i32,
        level: i32,
        optname: i32,
        optval: *mut std::ffi::c_void,
        optlen: *mut u32,
    ) -> i32;
}

pub fn peer_cred<T: AsRawFd>(stream: &T) -> std::io::Result<PeerCred> {
    peer_cred_fd(stream.as_raw_fd())
}

fn peer_cred_fd(fd: RawFd) -> std::io::Result<PeerCred> {
    let mut cred = PeerCred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut len = std::mem::size_of::<PeerCred>() as u32;
    // SAFETY: cred points to writable storage of exactly len bytes.
    let rc = unsafe {
        getsockopt(
            fd,
            SOL_SOCKET,
            SO_PEERCRED,
            (&mut cred as *mut PeerCred).cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    if len as usize != std::mem::size_of::<PeerCred>() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "SO_PEERCRED returned an unexpected size",
        ));
    }
    Ok(cred)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Operation {
    Status = 1,
    Setup = 2,
    Verify = 3,
}

impl Operation {
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Status),
            2 => Some(Self::Setup),
            3 => Some(Self::Verify),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    SetupRequired,
    Ready,
}

fn request(operation: Operation, payload: &[u8]) -> Result<String, String> {
    if payload.len() > MAX_PASSWORD_BYTES {
        return Err("credential payload too long".into());
    }
    let mut stream =
        UnixStream::connect(SOCKET_PATH).map_err(|e| format!("connect {SOCKET_PATH}: {e}"))?;
    let len = u32::try_from(payload.len()).map_err(|_| "credential payload too long")?;
    stream
        .write_all(&[operation as u8])
        .and_then(|_| stream.write_all(&len.to_be_bytes()))
        .and_then(|_| stream.write_all(payload))
        .map_err(|e| format!("write auth request: {e}"))?;
    let mut reply = Vec::new();
    stream
        .take(512)
        .read_to_end(&mut reply)
        .map_err(|e| format!("read auth reply: {e}"))?;
    let reply = String::from_utf8(reply).map_err(|_| "auth reply is not UTF-8")?;
    let reply = reply.trim().to_string();
    if let Some(reason) = reply.strip_prefix("ERR ") {
        return Err(reason.to_string());
    }
    Ok(reply)
}

pub fn status() -> Result<AuthStatus, String> {
    match request(Operation::Status, &[])?.as_str() {
        "OK setup-required" => Ok(AuthStatus::SetupRequired),
        "OK ready" => Ok(AuthStatus::Ready),
        other => Err(format!("unexpected auth status reply {other:?}")),
    }
}

pub fn setup(password: &str) -> Result<(), String> {
    validate_password(password)?;
    match request(Operation::Setup, password.as_bytes())?.as_str() {
        "OK setup" => Ok(()),
        other => Err(format!("unexpected auth setup reply {other:?}")),
    }
}

pub fn verify(password: &str) -> Result<bool, String> {
    if password.is_empty() || password.len() > MAX_PASSWORD_BYTES {
        return Ok(false);
    }
    match request(Operation::Verify, password.as_bytes())?.as_str() {
        "OK verified" => Ok(true),
        "OK invalid" => Ok(false),
        other => Err(format!("unexpected auth verify reply {other:?}")),
    }
}

pub fn validate_password(password: &str) -> Result<(), String> {
    let len = password.as_bytes().len();
    if !(MIN_PASSWORD_BYTES..=MAX_PASSWORD_BYTES).contains(&len) {
        return Err(format!(
            "password must be between {MIN_PASSWORD_BYTES} and {MAX_PASSWORD_BYTES} bytes"
        ));
    }
    if password.chars().any(|ch| ch.is_control()) {
        return Err("password must not contain control characters".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_are_strict() {
        assert_eq!(Operation::from_byte(1), Some(Operation::Status));
        assert_eq!(Operation::from_byte(3), Some(Operation::Verify));
        assert_eq!(Operation::from_byte(0), None);
        assert_eq!(Operation::from_byte(4), None);
    }

    #[test]
    fn password_policy_is_bounded() {
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password("short").is_err());
        assert!(validate_password("bad\npassword").is_err());
        assert!(validate_password(&"x".repeat(MAX_PASSWORD_BYTES + 1)).is_err());
    }
}
