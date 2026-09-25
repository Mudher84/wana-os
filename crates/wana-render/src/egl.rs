//! EGL on GBM: display, config, OpenGL ES context and window surface.

use crate::ffi::*;
use crate::gbm;
use std::ffi::CStr;
use std::ptr::null_mut;

fn egl_error(what: &str) -> String {
    // SAFETY: eglGetError has no preconditions.
    let code = unsafe { eglGetError() };
    format!("{what} failed (EGL error 0x{code:04x})")
}

/// Initialized EGL display for a GBM device, with a current ES context.
#[derive(Debug)]
pub struct Egl {
    display: EGLDisplay,
    context: EGLContext,
    surface: EGLSurface,
    pub version: (i32, i32),
}

impl Egl {
    /// EGL 1.5 platform display on GBM, ES2-compatible XRGB8888 config,
    /// context, window surface on `surface`, made current.
    pub fn new(device: &gbm::Device, surface: &gbm::Surface) -> Result<Egl, String> {
        // SAFETY: EGL calls with valid handles created in this function;
        // attribute lists are NONE-terminated arrays alive for each call.
        unsafe {
            let display = eglGetPlatformDisplay(
                EGL_PLATFORM_GBM_KHR,
                device.as_ptr().cast(),
                std::ptr::null(),
            );
            if display.is_null() {
                return Err(egl_error("eglGetPlatformDisplay(GBM)"));
            }
            let (mut major, mut minor) = (0, 0);
            if eglInitialize(display, &mut major, &mut minor) != EGL_TRUE {
                return Err(egl_error("eglInitialize"));
            }
            if eglBindAPI(EGL_OPENGL_ES_API) != EGL_TRUE {
                return Err(egl_error("eglBindAPI(OpenGL ES)"));
            }
            let config = choose_config(display)?;
            let ctx_attribs = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
            let context = eglCreateContext(display, config, null_mut(), ctx_attribs.as_ptr());
            if context.is_null() {
                return Err(egl_error("eglCreateContext(ES 2)"));
            }
            let egl_surface = eglCreatePlatformWindowSurface(
                display,
                config,
                surface.as_ptr().cast(),
                std::ptr::null(),
            );
            if egl_surface.is_null() {
                return Err(egl_error("eglCreatePlatformWindowSurface"));
            }
            if eglMakeCurrent(display, egl_surface, egl_surface, context) != EGL_TRUE {
                return Err(egl_error("eglMakeCurrent"));
            }
            Ok(Egl {
                display,
                context,
                surface: egl_surface,
                version: (major, minor),
            })
        }
    }

    pub fn query(&self, name: EGLint) -> String {
        // SAFETY: initialized display; returned string is owned by EGL.
        let p = unsafe { eglQueryString(self.display, name) };
        if p.is_null() {
            return "?".into();
        }
        // SAFETY: non-null NUL-terminated string.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }

    pub fn swap(&self) -> Result<(), String> {
        // SAFETY: current display/surface pair.
        if unsafe { eglSwapBuffers(self.display, self.surface) } != EGL_TRUE {
            return Err(egl_error("eglSwapBuffers"));
        }
        Ok(())
    }
}

/// Picks a window-capable ES2 config whose native visual is XRGB8888, as
/// required for scanout of the GBM surface.
unsafe fn choose_config(display: EGLDisplay) -> Result<EGLConfig, String> {
    let attribs = [
        EGL_SURFACE_TYPE,
        EGL_WINDOW_BIT,
        EGL_RED_SIZE,
        8,
        EGL_GREEN_SIZE,
        8,
        EGL_BLUE_SIZE,
        8,
        EGL_ALPHA_SIZE,
        0,
        EGL_RENDERABLE_TYPE,
        EGL_OPENGL_ES2_BIT,
        EGL_NONE,
    ];
    let mut configs = [null_mut(); 64];
    let mut n = 0;
    // SAFETY: `configs` has room for 64 entries, as passed.
    if unsafe { eglChooseConfig(display, attribs.as_ptr(), configs.as_mut_ptr(), 64, &mut n) }
        != EGL_TRUE
        || n == 0
    {
        return Err(egl_error("eglChooseConfig (no XRGB8888 ES2 window config)"));
    }
    for &cfg in &configs[..n as usize] {
        let mut visual = 0;
        // SAFETY: cfg came from eglChooseConfig on this display.
        unsafe { eglGetConfigAttrib(display, cfg, EGL_NATIVE_VISUAL_ID, &mut visual) };
        if visual as u32 == GBM_FORMAT_XRGB8888 {
            return Ok(cfg);
        }
    }
    Err("no EGL config with native visual XRGB8888".into())
}

impl Drop for Egl {
    fn drop(&mut self) {
        // SAFETY: tearing down objects created in new(), in reverse order.
        unsafe {
            eglMakeCurrent(self.display, null_mut(), null_mut(), null_mut());
            eglDestroySurface(self.display, self.surface);
            eglDestroyContext(self.display, self.context);
            eglTerminate(self.display);
        }
    }
}
