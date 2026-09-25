//! GBM (Generic Buffer Management): GPU buffers that KMS can scan out.

use crate::ffi::*;
use std::ffi::CStr;
use std::os::fd::RawFd;

/// A GBM device on a DRM fd. The fd must outlive the device.
#[derive(Debug)]
pub struct Device {
    raw: *mut gbm_device,
}

impl Device {
    pub fn new(drm_fd: RawFd) -> Result<Device, String> {
        // SAFETY: gbm_create_device only reads the fd; the caller keeps the
        // card (and its fd) alive for the device's lifetime.
        let raw = unsafe { gbm_create_device(drm_fd) };
        if raw.is_null() {
            return Err("gbm_create_device failed".into());
        }
        Ok(Device { raw })
    }

    pub fn as_ptr(&self) -> *mut gbm_device {
        self.raw
    }

    pub fn backend_name(&self) -> String {
        // SAFETY: valid device; the returned string is static in libgbm.
        let p = unsafe { gbm_device_get_backend_name(self.raw) };
        if p.is_null() {
            return "?".into();
        }
        // SAFETY: non-null, NUL-terminated string owned by libgbm.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: created by gbm_create_device; surfaces are dropped first
        // (they borrow the device).
        unsafe { gbm_device_destroy(self.raw) };
    }
}

/// A GBM surface: the chain of buffers EGL renders into.
#[derive(Debug)]
pub struct Surface<'d> {
    raw: *mut gbm_surface,
    _device: std::marker::PhantomData<&'d Device>,
}

/// A locked front buffer, ready for scanout. Must be released back to the
/// surface once the display no longer shows it.
#[derive(Debug, Clone, Copy)]
pub struct FrontBuffer {
    raw: *mut gbm_bo,
    pub handle: u32,
    pub stride: u32,
    pub width: u32,
    pub height: u32,
}

impl<'d> Surface<'d> {
    pub fn new(device: &'d Device, width: u32, height: u32) -> Result<Surface<'d>, String> {
        // SAFETY: valid device; flags/format are plain values.
        let raw = unsafe {
            gbm_surface_create(
                device.as_ptr(),
                width,
                height,
                GBM_FORMAT_XRGB8888,
                GBM_BO_USE_SCANOUT | GBM_BO_USE_RENDERING,
            )
        };
        if raw.is_null() {
            return Err(format!(
                "gbm_surface_create {width}x{height} XRGB8888 failed"
            ));
        }
        Ok(Surface {
            raw,
            _device: std::marker::PhantomData,
        })
    }

    pub fn as_ptr(&self) -> *mut gbm_surface {
        self.raw
    }

    /// Takes the buffer EGL just finished (call after eglSwapBuffers).
    pub fn lock_front(&self) -> Result<FrontBuffer, String> {
        // SAFETY: valid surface; must follow a swap, which callers ensure.
        let bo = unsafe { gbm_surface_lock_front_buffer(self.raw) };
        if bo.is_null() {
            return Err("gbm_surface_lock_front_buffer failed".into());
        }
        // SAFETY: bo is a valid, locked buffer object.
        unsafe {
            Ok(FrontBuffer {
                raw: bo,
                handle: gbm_bo_get_handle(bo).u32_,
                stride: gbm_bo_get_stride(bo),
                width: gbm_bo_get_width(bo),
                height: gbm_bo_get_height(bo),
            })
        }
    }

    pub fn release(&self, buf: FrontBuffer) {
        // SAFETY: buf was locked from this surface and not yet released.
        unsafe { gbm_surface_release_buffer(self.raw, buf.raw) };
    }
}

impl Drop for Surface<'_> {
    fn drop(&mut self) {
        // SAFETY: created by gbm_surface_create; EGL surface dropped first.
        unsafe { gbm_surface_destroy(self.raw) };
    }
}
