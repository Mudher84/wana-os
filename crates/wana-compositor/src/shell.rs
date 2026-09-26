//! Starting the shell (decision 0003): the compositor creates the shell's
//! Wayland connection itself, a socketpair, and hands the shell its end as
//! `WAYLAND_SOCKET`. That client is the only privileged one: it sees the
//! privileged globals (layer-shell), which do not exist for any client that
//! connects through the public socket. Nothing a client can send, and no
//! name or timing, makes it the shell.

use crate::globals::Compositor;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command};
use wana_log::{info, Subsystem};
use wana_wayland::server::Display;

const COMPOSITOR: Subsystem = Subsystem::Compositor;
const AF_UNIX: i32 = 1;
const SOCK_STREAM: i32 = 1;
const SOCK_CLOEXEC: i32 = 0o2_000_000;
const F_SETFD: i32 = 2;

extern "C" {
    fn socketpair(domain: i32, kind: i32, protocol: i32, sv: *mut [i32; 2]) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
}

/// Starts `argv` as the shell on a private, privileged connection.
pub fn start(
    display: &mut Display<Compositor>,
    argv: &[String],
    runtime_dir: &Path,
) -> Result<Child, String> {
    let mut sv = [-1i32; 2];
    // SAFETY: sv is a valid out array; both fds are close-on-exec so no
    // other child inherits them.
    if unsafe { socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, &mut sv) } != 0 {
        return Err(format!("socketpair: {}", std::io::Error::last_os_error()));
    }
    // SAFETY: fresh fds from socketpair, each owned exactly once here.
    let (ours, theirs) = unsafe { (OwnedFd::from_raw_fd(sv[0]), OwnedFd::from_raw_fd(sv[1])) };
    let shell_fd = sv[1];
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("XDG_RUNTIME_DIR", runtime_dir)
        .env("WAYLAND_SOCKET", shell_fd.to_string());
    // SAFETY: runs in the child between fork and exec; fcntl is
    // async-signal-safe. It clears close-on-exec on the shell's end only.
    unsafe {
        cmd.pre_exec(move || {
            if fcntl(shell_fd, F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        })
    };
    let child = cmd
        .spawn()
        .map_err(|e| format!("cannot start shell {}: {e}", argv[0]))?;
    drop(theirs); // the child has its copy
    display
        .add_privileged_client(ours)
        .map_err(|e| format!("shell connection: {e}"))?;
    info!(
        COMPOSITOR,
        "shell: started {} (pid {}) on a private connection; it alone sees the privileged globals",
        argv.join(" "),
        child.id()
    );
    Ok(child)
}
