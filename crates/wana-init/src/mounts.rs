//! Early filesystem mounts performed by PID 1 before anything else runs.

use crate::sys::{self, MS_NODEV, MS_NOEXEC, MS_NOSUID};
use std::io;
use std::os::raw::c_ulong;

/// One early mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mount {
    pub source: &'static str,
    pub target: &'static str,
    pub fstype: &'static str,
    pub flags: c_ulong,
    pub data: &'static str,
}

/// Early mounts, in order. Parents come before children (`/dev` before
/// `/dev/pts`). `devtmpfs` is included because the kernel does not
/// auto-mount it for an initramfs root.
pub const EARLY: &[Mount] = &[
    Mount {
        source: "proc",
        target: "/proc",
        fstype: "proc",
        flags: MS_NOSUID | MS_NODEV | MS_NOEXEC,
        data: "",
    },
    Mount {
        source: "sysfs",
        target: "/sys",
        fstype: "sysfs",
        flags: MS_NOSUID | MS_NODEV | MS_NOEXEC,
        data: "",
    },
    Mount {
        source: "devtmpfs",
        target: "/dev",
        fstype: "devtmpfs",
        flags: MS_NOSUID,
        data: "mode=0755",
    },
    Mount {
        source: "devpts",
        target: "/dev/pts",
        fstype: "devpts",
        flags: MS_NOSUID | MS_NOEXEC,
        data: "gid=5,mode=0620,ptmxmode=0666",
    },
    Mount {
        source: "tmpfs",
        target: "/dev/shm",
        fstype: "tmpfs",
        flags: MS_NOSUID | MS_NODEV,
        data: "mode=1777",
    },
    Mount {
        source: "tmpfs",
        target: "/run",
        fstype: "tmpfs",
        flags: MS_NOSUID | MS_NODEV,
        data: "mode=0755",
    },
    Mount {
        source: "tmpfs",
        target: "/tmp",
        fstype: "tmpfs",
        flags: MS_NOSUID | MS_NODEV,
        data: "mode=1777",
    },
];

const EBUSY: i32 = 16;

/// Outcome of one mount attempt.
#[derive(Debug)]
pub enum Outcome {
    Mounted,
    /// Something is already mounted there (e.g. the kernel mounted devtmpfs).
    AlreadyMounted,
    Failed(io::Error),
}

/// Mounts one entry, creating the mount point if it does not exist.
pub fn apply(m: &Mount) -> Outcome {
    if let Err(e) = std::fs::create_dir_all(m.target) {
        return Outcome::Failed(e);
    }
    match sys::mount_fs(m.source, m.target, m.fstype, m.flags, m.data) {
        Ok(()) => Outcome::Mounted,
        Err(e) if e.raw_os_error() == Some(EBUSY) => Outcome::AlreadyMounted,
        Err(e) => Outcome::Failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn targets_are_unique_and_absolute() {
        let mut seen = HashSet::new();
        for m in EARLY {
            assert!(m.target.starts_with('/'), "{} not absolute", m.target);
            assert!(seen.insert(m.target), "duplicate target {}", m.target);
        }
    }

    #[test]
    fn parents_are_mounted_before_children() {
        for (i, child) in EARLY.iter().enumerate() {
            for parent in &EARLY[i + 1..] {
                let nested = child.target.starts_with(parent.target)
                    && child.target.as_bytes().get(parent.target.len()) == Some(&b'/');
                assert!(
                    !nested,
                    "{} is mounted after its child {}",
                    parent.target, child.target
                );
            }
        }
    }

    #[test]
    fn kernel_interfaces_are_noexec() {
        for m in EARLY
            .iter()
            .filter(|m| matches!(m.fstype, "proc" | "sysfs"))
        {
            assert_ne!(m.flags & MS_NOEXEC, 0, "{} must be noexec", m.target);
            assert_ne!(m.flags & MS_NOSUID, 0, "{} must be nosuid", m.target);
        }
    }
}
