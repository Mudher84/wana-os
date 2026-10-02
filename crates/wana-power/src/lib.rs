use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::os::fd::{AsRawFd, RawFd};

pub const SOCKET_PATH: &str = "/run/wana/power.sock";
pub const DESKTOP_UID: u32 = 1000;
pub const DESKTOP_GID: u32 = 1000;
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

pub fn peer_cred<T: AsRawFd>(stream: &T) -> io::Result<PeerCred> {
    peer_cred_fd(stream.as_raw_fd())
}

fn peer_cred_fd(fd: RawFd) -> io::Result<PeerCred> {
    let mut cred = PeerCred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut len = std::mem::size_of::<PeerCred>() as u32;
    // SAFETY: cred points to writable storage of exactly len bytes and
    // getsockopt only writes that storage for SOL_SOCKET/SO_PEERCRED.
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
        return Err(io::Error::last_os_error());
    }
    if len as usize != std::mem::size_of::<PeerCred>() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "SO_PEERCRED returned an unexpected size",
        ));
    }
    Ok(cred)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Status,
    PowerOff,
    Reboot,
}

impl Command {
    pub fn parse(input: &str) -> Result<Self, &'static str> {
        match input.trim() {
            "status" => Ok(Self::Status),
            "poweroff" => Ok(Self::PowerOff),
            "reboot" => Ok(Self::Reboot),
            _ => Err("expected status|poweroff|reboot"),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::PowerOff => "poweroff",
            Self::Reboot => "reboot",
        }
    }
}

pub fn authorized(uid: u32) -> bool {
    uid == 0 || uid == DESKTOP_UID
}

fn request_line(line: &str) -> Result<String, String> {
    let mut stream = UnixStream::connect(SOCKET_PATH)
        .map_err(|e| format!("connect {SOCKET_PATH}: {e}"))?;
    stream
        .write_all(format!("{line}\n").as_bytes())
        .map_err(|e| format!("write {SOCKET_PATH}: {e}"))?;
    let mut reply = String::new();
    stream
        .take(256)
        .read_to_string(&mut reply)
        .map_err(|e| format!("read {SOCKET_PATH}: {e}"))?;
    let reply = reply.trim().to_string();
    if let Some(reason) = reply.strip_prefix("ERR ") {
        Err(reason.to_string())
    } else if reply.starts_with("OK ") {
        Ok(reply)
    } else {
        Err(format!("unexpected power broker reply {reply:?}"))
    }
}

pub fn request(command: Command) -> Result<String, String> {
    request_line(command.as_str())
}

pub fn request_from_ui(command: Command, compositor_pid: i32) -> Result<String, String> {
    if compositor_pid <= 0 {
        return Err("invalid compositor pid".into());
    }
    request_line(&format!("{} {compositor_pid}", command.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_parser_is_strict() {
        assert_eq!(Command::parse("status\n"), Ok(Command::Status));
        assert_eq!(Command::parse("poweroff"), Ok(Command::PowerOff));
        assert_eq!(Command::parse("reboot"), Ok(Command::Reboot));
        assert!(Command::parse("shutdown").is_err());
        assert!(Command::parse("reboot now").is_err());
    }

    #[test]
    fn only_root_and_desktop_uid_are_authorized() {
        assert!(authorized(0));
        assert!(authorized(DESKTOP_UID));
        assert!(!authorized(1));
        assert!(!authorized(999));
        assert!(!authorized(1001));
    }
}
