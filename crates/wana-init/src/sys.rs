//! Minimal bindings to the libc calls PID 1 needs that `std` does not expose.
//!
//! Every function here is a thin, safe wrapper: arguments are converted to
//! owned C strings before the call and errors come back as `io::Error`.

use std::ffi::{CString, NulError};
use std::io;
use std::os::raw::{c_char, c_int, c_ulong, c_void};

pub const MS_RDONLY: c_ulong = 1;
pub const MS_NOSUID: c_ulong = 2;
pub const MS_NODEV: c_ulong = 4;
pub const MS_NOEXEC: c_ulong = 8;
const MS_REMOUNT: c_ulong = 32;
const MS_BIND: c_ulong = 4096;

const RB_POWER_OFF: c_int = 0x4321_fedc_u32 as c_int;
const RB_AUTOBOOT: c_int = 0x0123_4567;

const WNOHANG: c_int = 1;
const ECHILD: i32 = 10;
const EINTR: i32 = 4;

extern "C" {
    fn mount(
        source: *const c_char,
        target: *const c_char,
        fstype: *const c_char,
        flags: c_ulong,
        data: *const c_void,
    ) -> c_int;
    fn umount2(target: *const c_char, flags: c_int) -> c_int;
    fn sethostname(name: *const c_char, len: usize) -> c_int;
    fn chown(path: *const c_char, owner: u32, group: u32) -> c_int;
    fn reboot(cmd: c_int) -> c_int;
    fn sync();
    fn waitpid(pid: i32, status: *mut c_int, options: c_int) -> i32;
}

fn cstr(s: &str) -> io::Result<CString> {
    CString::new(s).map_err(|e: NulError| io::Error::new(io::ErrorKind::InvalidInput, e))
}

/// mount(2).
pub fn mount_fs(
    source: &str,
    target: &str,
    fstype: &str,
    flags: c_ulong,
    data: &str,
) -> io::Result<()> {
    let (source, target, fstype, data) = (cstr(source)?, cstr(target)?, cstr(fstype)?, cstr(data)?);
    // SAFETY: all pointers come from live CStrings that outlive the call;
    // mount(2) only reads them.
    let rc = unsafe {
        mount(
            source.as_ptr(),
            target.as_ptr(),
            fstype.as_ptr(),
            flags,
            data.as_ptr().cast(),
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// bind mount(2): expose an already-mounted persistent directory at the
/// conventional system path without changing the underlying filesystem.
pub fn bind_mount(source: &str, target: &str) -> io::Result<()> {
    let (source, target) = (cstr(source)?, cstr(target)?);
    // SAFETY: source/target are live C strings; fstype/data are ignored for
    // MS_BIND and may be null.
    let rc = unsafe {
        mount(
            source.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            MS_BIND,
            std::ptr::null(),
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Remount an already-mounted filesystem read-write without changing
/// its source, type or other mount options.
pub fn remount_rw(target: &str) -> io::Result<()> {
    let target = cstr(target)?;
    // SAFETY: source, fstype and data are ignored by MS_REMOUNT and may be
    // null. target is a live NUL-terminated CString.
    let rc = unsafe {
        mount(
            std::ptr::null(),
            target.as_ptr(),
            std::ptr::null(),
            MS_REMOUNT,
            std::ptr::null(),
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// umount2(2) without lazy/force flags.
pub fn unmount(target: &str) -> io::Result<()> {
    let target = cstr(target)?;
    // SAFETY: target is a live C string and flags=0 requests a normal unmount.
    let rc = unsafe { umount2(target.as_ptr(), 0) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// sethostname(2).
pub fn set_hostname(name: &str) -> io::Result<()> {
    // SAFETY: the pointer/length pair describes the bytes of `name`, which is
    // borrowed for the whole call; the kernel copies them.
    let rc = unsafe { sethostname(name.as_ptr().cast(), name.len()) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// chown(2).
pub fn chown_path(path: &str, uid: u32, gid: u32) -> io::Result<()> {
    let path = cstr(path)?;
    // SAFETY: path is a live NUL-terminated CString and chown only reads it.
    let rc = unsafe { chown(path.as_ptr(), uid, gid) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// How the system should stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Halt {
    PowerOff,
    Reboot,
}

/// Flushes filesystems and stops the machine. Only returns on failure.
pub fn halt(how: Halt) -> io::Error {
    let cmd = match how {
        Halt::PowerOff => RB_POWER_OFF,
        Halt::Reboot => RB_AUTOBOOT,
    };
    // SAFETY: sync(2) takes no arguments and cannot fail.
    unsafe { sync() };
    // SAFETY: reboot(2) with a valid command constant; on success it does
    // not return.
    unsafe { reboot(cmd) };
    io::Error::last_os_error()
}

/// Result of reaping one child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaped {
    /// A child exited; its pid and raw wait status.
    Child { pid: i32, status: i32 },
    /// Children exist but none has exited (only with `block == false`).
    Nothing,
    /// There are no children at all.
    NoChildren,
}

/// waitpid(-1, ...): reaps any child, including orphans re-parented to PID 1.
pub fn reap_any(block: bool) -> io::Result<Reaped> {
    let options = if block { 0 } else { WNOHANG };
    loop {
        let mut status: c_int = 0;
        // SAFETY: `status` is a valid, writable c_int for the duration of the call.
        let pid = unsafe { waitpid(-1, &mut status, options) };
        return match pid {
            0 => Ok(Reaped::Nothing),
            p if p > 0 => Ok(Reaped::Child { pid: p, status }),
            _ => {
                let err = io::Error::last_os_error();
                match err.raw_os_error() {
                    Some(EINTR) => continue,
                    Some(ECHILD) => Ok(Reaped::NoChildren),
                    _ => Err(err),
                }
            }
        };
    }
}
