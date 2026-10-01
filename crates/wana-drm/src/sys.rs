//! Kernel DRM uAPI: ioctl numbers and `repr(C)` structs from
//! `<drm/drm.h>` and `<drm/drm_mode.h>` (Linux 6.18), plus the few libc
//! calls needed to use them. Layouts are checked against the C headers in
//! the tests at the bottom of this file.

use std::io;
use std::os::fd::RawFd;
use std::os::raw::{c_int, c_ulong, c_void};

const DRM_IOCTL_BASE: c_ulong = b'd' as c_ulong;
const IOC_WRITE: c_ulong = 1;
const IOC_READ: c_ulong = 2;

const fn ioc(dir: c_ulong, nr: c_ulong, size: usize) -> c_ulong {
    (dir << 30) | ((size as c_ulong) << 16) | (DRM_IOCTL_BASE << 8) | nr
}
const fn iowr<T>(nr: c_ulong) -> c_ulong {
    ioc(IOC_READ | IOC_WRITE, nr, std::mem::size_of::<T>())
}
const fn iow<T>(nr: c_ulong) -> c_ulong {
    ioc(IOC_WRITE, nr, std::mem::size_of::<T>())
}

pub const DRM_IOCTL_VERSION: c_ulong = iowr::<DrmVersion>(0x00);
pub const DRM_IOCTL_GET_CAP: c_ulong = iowr::<DrmGetCap>(0x0c);
pub const DRM_IOCTL_SET_CLIENT_CAP: c_ulong = iow::<DrmSetClientCap>(0x0d);
pub const DRM_IOCTL_MODE_GETRESOURCES: c_ulong = iowr::<DrmModeCardRes>(0xA0);
pub const DRM_IOCTL_MODE_GETCRTC: c_ulong = iowr::<DrmModeCrtc>(0xA1);
pub const DRM_IOCTL_MODE_SETCRTC: c_ulong = iowr::<DrmModeCrtc>(0xA2);
pub const DRM_IOCTL_MODE_GETENCODER: c_ulong = iowr::<DrmModeGetEncoder>(0xA6);
pub const DRM_IOCTL_MODE_GETCONNECTOR: c_ulong = iowr::<DrmModeGetConnector>(0xA7);
pub const DRM_IOCTL_MODE_ADDFB: c_ulong = iowr::<DrmModeFbCmd>(0xAE);
pub const DRM_IOCTL_MODE_RMFB: c_ulong = iowr::<u32>(0xAF);
pub const DRM_IOCTL_MODE_PAGE_FLIP: c_ulong = iowr::<DrmModeCrtcPageFlip>(0xB0);
pub const DRM_IOCTL_MODE_CREATE_DUMB: c_ulong = iowr::<DrmModeCreateDumb>(0xB2);
pub const DRM_IOCTL_MODE_MAP_DUMB: c_ulong = iowr::<DrmModeMapDumb>(0xB3);
pub const DRM_IOCTL_MODE_DESTROY_DUMB: c_ulong = iowr::<DrmModeDestroyDumb>(0xB4);

pub const DRM_CAP_DUMB_BUFFER: u64 = 0x1;
pub const DRM_CAP_TIMESTAMP_MONOTONIC: u64 = 0x6;
pub const DRM_CLIENT_CAP_UNIVERSAL_PLANES: u64 = 2;

pub const DRM_MODE_TYPE_PREFERRED: u32 = 1 << 3;
pub const DRM_MODE_PAGE_FLIP_EVENT: u32 = 0x01;
pub const DRM_EVENT_FLIP_COMPLETE: u32 = 0x02;

pub const DRM_DISPLAY_MODE_LEN: usize = 32;

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmVersion {
    pub version_major: c_int,
    pub version_minor: c_int,
    pub version_patchlevel: c_int,
    pub name_len: usize,
    pub name: u64,
    pub date_len: usize,
    pub date: u64,
    pub desc_len: usize,
    pub desc: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmGetCap {
    pub capability: u64,
    pub value: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmSetClientCap {
    pub capability: u64,
    pub value: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmModeCardRes {
    pub fb_id_ptr: u64,
    pub crtc_id_ptr: u64,
    pub connector_id_ptr: u64,
    pub encoder_id_ptr: u64,
    pub count_fbs: u32,
    pub count_crtcs: u32,
    pub count_connectors: u32,
    pub count_encoders: u32,
    pub min_width: u32,
    pub max_width: u32,
    pub min_height: u32,
    pub max_height: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrmModeModeInfo {
    pub clock: u32,
    pub hdisplay: u16,
    pub hsync_start: u16,
    pub hsync_end: u16,
    pub htotal: u16,
    pub hskew: u16,
    pub vdisplay: u16,
    pub vsync_start: u16,
    pub vsync_end: u16,
    pub vtotal: u16,
    pub vscan: u16,
    pub vrefresh: u32,
    pub flags: u32,
    pub type_: u32,
    pub name: [u8; DRM_DISPLAY_MODE_LEN],
}

impl Default for DrmModeModeInfo {
    fn default() -> Self {
        // SAFETY: an all-zero bit pattern is a valid value for every field
        // (plain integers and a byte array).
        unsafe { std::mem::zeroed() }
    }
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmModeCrtc {
    pub set_connectors_ptr: u64,
    pub count_connectors: u32,
    pub crtc_id: u32,
    pub fb_id: u32,
    pub x: u32,
    pub y: u32,
    pub gamma_size: u32,
    pub mode_valid: u32,
    pub mode: DrmModeModeInfo,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmModeGetEncoder {
    pub encoder_id: u32,
    pub encoder_type: u32,
    pub crtc_id: u32,
    pub possible_crtcs: u32,
    pub possible_clones: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmModeGetConnector {
    pub encoders_ptr: u64,
    pub modes_ptr: u64,
    pub props_ptr: u64,
    pub prop_values_ptr: u64,
    pub count_modes: u32,
    pub count_props: u32,
    pub count_encoders: u32,
    pub encoder_id: u32,
    pub connector_id: u32,
    pub connector_type: u32,
    pub connector_type_id: u32,
    pub connection: u32,
    pub mm_width: u32,
    pub mm_height: u32,
    pub subpixel: u32,
    pub pad: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmModeFbCmd {
    pub fb_id: u32,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u32,
    pub depth: u32,
    pub handle: u32,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmModeCrtcPageFlip {
    pub crtc_id: u32,
    pub fb_id: u32,
    pub flags: u32,
    pub reserved: u32,
    pub user_data: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmModeCreateDumb {
    pub height: u32,
    pub width: u32,
    pub bpp: u32,
    pub flags: u32,
    pub handle: u32,
    pub pitch: u32,
    pub size: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmModeMapDumb {
    pub handle: u32,
    pub pad: u32,
    pub offset: u64,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct DrmModeDestroyDumb {
    pub handle: u32,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct DrmEvent {
    pub type_: u32,
    pub length: u32,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct DrmEventVblank {
    pub base: DrmEvent,
    pub user_data: u64,
    pub tv_sec: u32,
    pub tv_usec: u32,
    pub sequence: u32,
    pub crtc_id: u32,
}

const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const MAP_SHARED: c_int = 1;
const EINTR: i32 = 4;
const EAGAIN: i32 = 11;

extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        off: i64,
    ) -> *mut c_void;
    fn munmap(addr: *mut c_void, len: usize) -> c_int;
}

/// Issues a DRM ioctl, retrying on EINTR/EAGAIN as libdrm's drmIoctl does.
///
/// # Safety
/// `arg` must be the struct type the kernel expects for `request`, and any
/// user pointers stored inside it must point to buffers that stay valid and
/// large enough for the duration of the call.
pub unsafe fn drm_ioctl<T>(fd: RawFd, request: c_ulong, arg: &mut T) -> io::Result<()> {
    loop {
        // SAFETY: forwarded from the caller's contract; `arg` is a valid,
        // exclusively borrowed `T` for the whole call.
        let rc = unsafe { ioctl(fd, request, arg as *mut T) };
        if rc == 0 {
            return Ok(());
        }
        let err = io::Error::last_os_error();
        match err.raw_os_error() {
            Some(EINTR) | Some(EAGAIN) => continue,
            _ => return Err(err),
        }
    }
}

/// A shared, writable memory mapping of a DRM buffer, unmapped on drop.
#[derive(Debug)]
pub struct Mapping {
    ptr: *mut u8,
    len: usize,
}

impl Mapping {
    /// Maps `len` bytes of `fd` at the fake offset returned by MAP_DUMB.
    pub fn new(fd: RawFd, offset: u64, len: usize) -> io::Result<Mapping> {
        // SAFETY: a fresh mapping (addr = NULL) of a DRM buffer object; the
        // kernel validates fd, offset and length.
        let ptr = unsafe {
            mmap(
                std::ptr::null_mut(),
                len,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                fd,
                offset as i64,
            )
        };
        if ptr as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(Mapping {
            ptr: ptr.cast(),
            len,
        })
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: ptr/len describe a live mapping owned by self; the
        // exclusive borrow of self prevents aliasing through this API.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: unmapping exactly the region returned by mmap in new().
        unsafe { munmap(self.ptr.cast(), self.len) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    // Expected values: sizeof()/DRM_IOCTL_* from <drm/drm.h> and
    // <drm/drm_mode.h>, compiled for x86_64 (see docs/test-reports/phase-07.md).
    #[test]
    fn struct_sizes_match_kernel_headers() {
        assert_eq!(size_of::<DrmVersion>(), 64);
        assert_eq!(size_of::<DrmGetCap>(), 16);
        assert_eq!(size_of::<DrmSetClientCap>(), 16);
        assert_eq!(size_of::<DrmModeCardRes>(), 64);
        assert_eq!(size_of::<DrmModeModeInfo>(), 68);
        assert_eq!(size_of::<DrmModeCrtc>(), 104);
        assert_eq!(size_of::<DrmModeGetEncoder>(), 20);
        assert_eq!(size_of::<DrmModeGetConnector>(), 80);
        assert_eq!(size_of::<DrmModeFbCmd>(), 28);
        assert_eq!(size_of::<DrmModeCrtcPageFlip>(), 24);
        assert_eq!(size_of::<DrmModeCreateDumb>(), 32);
        assert_eq!(size_of::<DrmModeMapDumb>(), 16);
        assert_eq!(size_of::<DrmModeDestroyDumb>(), 4);
        assert_eq!(size_of::<DrmEvent>(), 8);
        assert_eq!(size_of::<DrmEventVblank>(), 32);
    }

    #[test]
    fn ioctl_numbers_match_kernel_headers() {
        assert_eq!(DRM_IOCTL_VERSION, 0xc040_6400);
        assert_eq!(DRM_IOCTL_GET_CAP, 0xc010_640c);
        assert_eq!(DRM_IOCTL_SET_CLIENT_CAP, 0x4010_640d);
        assert_eq!(DRM_IOCTL_MODE_GETRESOURCES, 0xc040_64a0);
        assert_eq!(DRM_IOCTL_MODE_GETCRTC, 0xc068_64a1);
        assert_eq!(DRM_IOCTL_MODE_SETCRTC, 0xc068_64a2);
        assert_eq!(DRM_IOCTL_MODE_GETENCODER, 0xc014_64a6);
        assert_eq!(DRM_IOCTL_MODE_GETCONNECTOR, 0xc050_64a7);
        assert_eq!(DRM_IOCTL_MODE_ADDFB, 0xc01c_64ae);
        assert_eq!(DRM_IOCTL_MODE_RMFB, 0xc004_64af);
        assert_eq!(DRM_IOCTL_MODE_PAGE_FLIP, 0xc018_64b0);
        assert_eq!(DRM_IOCTL_MODE_CREATE_DUMB, 0xc020_64b2);
        assert_eq!(DRM_IOCTL_MODE_MAP_DUMB, 0xc010_64b3);
        assert_eq!(DRM_IOCTL_MODE_DESTROY_DUMB, 0xc004_64b4);
    }
}
