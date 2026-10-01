//! wl_shm pools and buffers: client memory mapped read-only, copied out at
//! commit time under a SIGBUS guard.
//!
//! A client may shrink the file behind a pool after creating it. Touching
//! the missing pages then raises SIGBUS, which would kill the compositor.
//! While a pool is being read, the faulting range is registered here. The
//! SIGBUS handler maps anonymous zero pages over that range (MAP_FIXED), the
//! faulting instruction restarts and reads zeros, and the read reports the
//! pool as poisoned. The compositor then terminates that client with
//! `wl_shm.error(invalid_fd)`. A SIGBUS outside the registered range is not
//! ours: the handler restores the default action and the fault repeats, so
//! real bugs still crash visibly. libwayland's own shm code works the same
//! way (`wl_shm_buffer_begin_access`).

use std::fs::File;
use std::os::raw::{c_int, c_void};
use std::os::unix::io::{AsRawFd, OwnedFd};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, Once};

/// wl_shm.format values accepted (little-endian 32-bit).
pub const FORMAT_ARGB8888: u32 = 0;
pub const FORMAT_XRGB8888: u32 = 1;

/// Why a pool or buffer request is refused, with its wl_shm error code.
// Variant names follow the protocol's error names (invalid_format, ...).
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShmError {
    /// `wl_shm.error.invalid_format` (0).
    InvalidFormat(String),
    /// `wl_shm.error.invalid_stride` (1).
    InvalidStride(String),
    /// `wl_shm.error.invalid_fd` (2).
    InvalidFd(String),
}

impl ShmError {
    pub fn code(&self) -> u32 {
        match self {
            ShmError::InvalidFormat(_) => 0,
            ShmError::InvalidStride(_) => 1,
            ShmError::InvalidFd(_) => 2,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            ShmError::InvalidFormat(m) | ShmError::InvalidStride(m) | ShmError::InvalidFd(m) => m,
        }
    }
}

// --- libc -------------------------------------------------------------------
const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const MAP_SHARED: c_int = 0x01;
const MAP_PRIVATE: c_int = 0x02;
const MAP_FIXED: c_int = 0x10;
const MAP_ANONYMOUS: c_int = 0x20;
const SIGBUS: c_int = 7;
const SA_SIGINFO: c_int = 4;
const SA_RESTART: c_int = 0x1000_0000;
const SIG_DFL: usize = 0;

/// glibc `struct sigaction` on x86_64.
#[repr(C)]
struct SigAction {
    handler: usize,
    mask: [u64; 16],
    flags: c_int,
    restorer: usize,
}

/// The leading fields of `siginfo_t` (x86_64): si_addr is at offset 16.
#[repr(C)]
struct SigInfo {
    signo: c_int,
    errno: c_int,
    code: c_int,
    _pad: c_int,
    addr: *mut c_void,
}

extern "C" {
    fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        off: i64,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
    fn sigaction(sig: c_int, act: *const SigAction, old: *mut SigAction) -> c_int;
}

fn map_failed(p: *mut c_void) -> bool {
    p as isize == -1
}

// --- SIGBUS guard -------------------------------------------------------------
static GUARD_START: AtomicUsize = AtomicUsize::new(0);
static GUARD_LEN: AtomicUsize = AtomicUsize::new(0);
static FAULTED: AtomicBool = AtomicBool::new(false);
/// One guarded read at a time (the compositor is single-threaded; tests
/// are not).
static GUARD_LOCK: Mutex<()> = Mutex::new(());
static INSTALL: Once = Once::new();

unsafe extern "C" fn on_sigbus(_sig: c_int, info: *mut SigInfo, _ctx: *mut c_void) {
    // Only async-signal-safe operations: atomics, mmap, sigaction.
    // SAFETY: the kernel passes a valid siginfo_t for SA_SIGINFO handlers.
    let addr = unsafe { (*info).addr } as usize;
    let start = GUARD_START.load(Ordering::SeqCst);
    let len = GUARD_LEN.load(Ordering::SeqCst);
    if start != 0 && addr >= start && addr < start + len {
        // SAFETY: replaces exactly the registered mapping with private zero
        // pages; the read in progress restarts on them.
        let p = unsafe {
            mmap(
                start as *mut c_void,
                len,
                PROT_READ | PROT_WRITE,
                MAP_FIXED | MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if !map_failed(p) {
            FAULTED.store(true, Ordering::SeqCst);
            return;
        }
    }
    // Not a guarded pool read: let the fault kill the process as usual.
    let dfl = SigAction {
        handler: SIG_DFL,
        mask: [0; 16],
        flags: 0,
        restorer: 0,
    };
    // SAFETY: valid struct; restoring the default action is async-signal-safe.
    unsafe { sigaction(SIGBUS, &dfl, ptr::null_mut()) };
}

fn install_guard() {
    INSTALL.call_once(|| {
        let act = SigAction {
            handler: on_sigbus as *const () as usize,
            mask: [0; 16],
            flags: SA_SIGINFO | SA_RESTART,
            restorer: 0,
        };
        // SAFETY: valid handler with the SA_SIGINFO signature.
        let rc = unsafe { sigaction(SIGBUS, &act, ptr::null_mut()) };
        assert_eq!(rc, 0, "sigaction(SIGBUS)");
    });
}

/// A client's pool: its file, mapped read-only and shared.
#[derive(Debug)]
pub struct Pool {
    file: File,
    addr: usize,
    size: usize,
    poisoned: bool,
}

impl Pool {
    /// Maps `size` bytes of the client's file.
    pub fn new(fd: OwnedFd, size: i32) -> Result<Pool, ShmError> {
        install_guard();
        let file = File::from(fd);
        let mut pool = Pool {
            file,
            addr: 0,
            size: 0,
            poisoned: false,
        };
        pool.map(size)?;
        Ok(pool)
    }

    fn map(&mut self, size: i32) -> Result<(), ShmError> {
        if size <= 0 {
            return Err(ShmError::InvalidStride(format!(
                "pool size {size} must be positive"
            )));
        }
        let size = size as usize;
        let file_len = self
            .file
            .metadata()
            .map_err(|e| ShmError::InvalidFd(format!("fstat: {e}")))?
            .len();
        if (file_len as u128) < size as u128 {
            return Err(ShmError::InvalidFd(format!(
                "pool size {size} exceeds the file size {file_len}"
            )));
        }
        // SAFETY: a new read-only shared mapping of a file we own the fd of.
        let p = unsafe {
            mmap(
                ptr::null_mut(),
                size,
                PROT_READ,
                MAP_SHARED,
                self.file.as_raw_fd(),
                0,
            )
        };
        if map_failed(p) {
            return Err(ShmError::InvalidFd(format!(
                "mmap {size} bytes: {}",
                std::io::Error::last_os_error()
            )));
        }
        self.unmap();
        self.addr = p as usize;
        self.size = size;
        Ok(())
    }

    fn unmap(&mut self) {
        if self.addr != 0 {
            // SAFETY: the mapping was created by `map` and is not in use.
            unsafe { munmap(self.addr as *mut c_void, self.size) };
            self.addr = 0;
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }

    /// `wl_shm_pool.resize`: pools may only grow.
    pub fn resize(&mut self, size: i32) -> Result<(), ShmError> {
        if size < 0 || (size as usize) < self.size {
            return Err(ShmError::InvalidStride(format!(
                "pool can only grow ({} -> {size})",
                self.size
            )));
        }
        self.map(size)
    }

    /// Copies a buffer out of the pool with rows packed (`width * 4` bytes
    /// each). Fails if the client shrank the file under the pool.
    pub fn read(&mut self, b: &BufferLayout) -> Result<Vec<u8>, ShmError> {
        if self.poisoned {
            return Err(ShmError::InvalidFd("pool file was truncated".into()));
        }
        b.check(self.size)?;
        let row = b.width as usize * 4;
        let mut out = vec![0u8; row * b.height as usize];
        let _lock = GUARD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        FAULTED.store(false, Ordering::SeqCst);
        GUARD_LEN.store(self.size, Ordering::SeqCst);
        GUARD_START.store(self.addr, Ordering::SeqCst);
        for y in 0..b.height as usize {
            let src = self.addr + b.offset as usize + y * b.stride as usize;
            // SAFETY: `check` proved offset + stride*(height-1) + width*4 <=
            // pool size, so each row lies inside the mapping; a SIGBUS from a
            // truncated file is absorbed by the guard (reads become zeros).
            unsafe {
                ptr::copy_nonoverlapping(src as *const u8, out.as_mut_ptr().add(y * row), row);
            }
        }
        GUARD_START.store(0, Ordering::SeqCst);
        if FAULTED.swap(false, Ordering::SeqCst) {
            // The mapping is now anonymous zero pages; never read it again.
            self.poisoned = true;
            return Err(ShmError::InvalidFd(
                "pool file was truncated while in use (SIGBUS)".into(),
            ));
        }
        Ok(out)
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.unmap();
    }
}

/// Where a wl_buffer lives in its pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferLayout {
    pub offset: i32,
    pub width: i32,
    pub height: i32,
    pub stride: i32,
    pub format: u32,
}

impl BufferLayout {
    /// Validates the layout against a pool of `pool_size` bytes, with
    /// overflow-checked arithmetic.
    pub fn check(&self, pool_size: usize) -> Result<(), ShmError> {
        if self.format != FORMAT_ARGB8888 && self.format != FORMAT_XRGB8888 {
            return Err(ShmError::InvalidFormat(format!(
                "format 0x{:x}",
                self.format
            )));
        }
        if self.width <= 0 || self.height <= 0 || self.offset < 0 {
            return Err(ShmError::InvalidStride(format!(
                "invalid buffer {}x{} at offset {}",
                self.width, self.height, self.offset
            )));
        }
        let row = (self.width as u64) * 4;
        if (self.stride as i64) < row as i64 {
            return Err(ShmError::InvalidStride(format!(
                "stride {} < width {} * 4",
                self.stride, self.width
            )));
        }
        let end = (self.offset as u64) + (self.stride as u64) * (self.height as u64 - 1) + row;
        if end > pool_size as u64 {
            return Err(ShmError::InvalidStride(format!(
                "buffer ends at byte {end}, pool has {pool_size}"
            )));
        }
        Ok(())
    }

    pub fn opaque(&self) -> bool {
        self.format == FORMAT_XRGB8888
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::io::FromRawFd;

    extern "C" {
        fn memfd_create(name: *const std::os::raw::c_char, flags: u32) -> c_int;
    }

    fn memfd(bytes: &[u8]) -> File {
        // SAFETY: NUL-terminated name; the returned fd is owned by the File.
        let fd = unsafe { memfd_create(c"wana-shm-test".as_ptr(), 0) };
        assert!(fd >= 0);
        // SAFETY: fresh fd from memfd_create.
        let mut f = unsafe { File::from_raw_fd(fd) };
        f.write_all(bytes).unwrap();
        f
    }

    fn layout(offset: i32, w: i32, h: i32, stride: i32) -> BufferLayout {
        BufferLayout {
            offset,
            width: w,
            height: h,
            stride,
            format: FORMAT_XRGB8888,
        }
    }

    #[test]
    fn layouts_are_validated_without_overflow() {
        assert!(layout(0, 2, 2, 8).check(16).is_ok());
        assert!(layout(0, 2, 2, 8).check(15).is_err(), "one byte short");
        assert_eq!(
            layout(0, 2, 2, 7).check(64).unwrap_err().code(),
            1,
            "stride < width*4"
        );
        assert_eq!(layout(-4, 2, 2, 8).check(64).unwrap_err().code(), 1);
        assert_eq!(layout(0, 0, 2, 8).check(64).unwrap_err().code(), 1);
        // The end offset (~2^62) is computed in u64 without overflow and is
        // refused against the largest possible pool (create_pool takes i32).
        let huge = layout(i32::MAX, i32::MAX / 4, i32::MAX, i32::MAX);
        assert_eq!(huge.check(i32::MAX as usize).unwrap_err().code(), 1);
        let mut bad = layout(0, 1, 1, 4);
        bad.format = 0x3432_5258 + 1;
        assert_eq!(bad.check(64).unwrap_err().code(), 0);
        // Last row may be shorter than the stride.
        assert!(layout(4, 1, 2, 16).check(4 + 16 + 4).is_ok());
    }

    #[test]
    fn rows_are_packed_out_of_a_strided_pool() {
        // 2x2 pixels, stride 12 (4 bytes of padding per row), offset 4.
        let mut bytes = vec![0xEEu8; 4];
        bytes.extend([1, 2, 3, 4, 5, 6, 7, 8, 0xAA, 0xAA, 0xAA, 0xAA]);
        bytes.extend([9, 10, 11, 12, 13, 14, 15, 16]);
        let f = memfd(&bytes);
        let mut pool = Pool::new(OwnedFd::from(f), bytes.len() as i32).unwrap();
        let out = pool.read(&layout(4, 2, 2, 12)).unwrap();
        assert_eq!(out, (1..=16).collect::<Vec<u8>>());
    }

    #[test]
    fn pool_larger_than_the_file_is_refused() {
        let f = memfd(&[0; 100]);
        let err = Pool::new(OwnedFd::from(f), 4096).unwrap_err();
        assert_eq!(err.code(), 2);
    }

    #[test]
    fn pools_only_grow() {
        let f = memfd(&[0; 8192]);
        let mut pool = Pool::new(OwnedFd::from(f), 4096).unwrap();
        assert!(pool.resize(8192).is_ok());
        assert_eq!(pool.size(), 8192);
        assert!(pool.resize(4096).is_err());
    }

    /// The client truncates the file after creating the pool: reading the
    /// buffer raises SIGBUS inside the guard. The compositor survives, the
    /// read fails with invalid_fd, and the pool stays poisoned.
    #[test]
    fn truncated_pool_is_caught_not_fatal() {
        let f = memfd(&vec![7u8; 3 * 4096]);
        let dup = f.try_clone().unwrap();
        let mut pool = Pool::new(OwnedFd::from(f), 3 * 4096).unwrap();
        dup.set_len(0).unwrap();
        let err = pool.read(&layout(0, 1024, 3, 4096)).unwrap_err();
        assert_eq!(err.code(), 2);
        assert!(err.message().contains("SIGBUS"), "{err:?}");
        assert!(pool.read(&layout(0, 1, 1, 4)).is_err(), "poisoned");
    }
}
