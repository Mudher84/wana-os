//! Minimal FFI declarations for libgbm, libEGL (1.5) and libGLESv2.
//! Constants are the Khronos/Mesa header values (gbm.h, EGL/egl.h,
//! GLES2/gl2.h); only what the renderer uses is declared.

#![allow(non_camel_case_types, non_snake_case)]

use std::os::raw::{c_char, c_int, c_uint, c_void};

// --- GBM ------------------------------------------------------------------
#[derive(Debug)]
pub enum gbm_device {}
#[derive(Debug)]
pub enum gbm_surface {}
#[derive(Debug)]
pub enum gbm_bo {}

#[repr(C)]
#[derive(Clone, Copy)]
pub union gbm_bo_handle {
    pub ptr: *mut c_void,
    pub s32: i32,
    pub u32_: u32,
    pub s64: i64,
    pub u64_: u64,
}

/// DRM_FORMAT_XRGB8888 = fourcc('X','R','2','4').
pub const GBM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
pub const GBM_BO_USE_SCANOUT: u32 = 1 << 0;
pub const GBM_BO_USE_RENDERING: u32 = 1 << 2;

#[link(name = "gbm")]
extern "C" {
    pub fn gbm_create_device(fd: c_int) -> *mut gbm_device;
    pub fn gbm_device_destroy(gbm: *mut gbm_device);
    pub fn gbm_device_get_backend_name(gbm: *mut gbm_device) -> *const c_char;
    pub fn gbm_surface_create(
        gbm: *mut gbm_device,
        width: u32,
        height: u32,
        format: u32,
        flags: u32,
    ) -> *mut gbm_surface;
    pub fn gbm_surface_destroy(surface: *mut gbm_surface);
    pub fn gbm_surface_lock_front_buffer(surface: *mut gbm_surface) -> *mut gbm_bo;
    pub fn gbm_surface_release_buffer(surface: *mut gbm_surface, bo: *mut gbm_bo);
    pub fn gbm_bo_get_handle(bo: *mut gbm_bo) -> gbm_bo_handle;
    pub fn gbm_bo_get_stride(bo: *mut gbm_bo) -> u32;
    pub fn gbm_bo_get_width(bo: *mut gbm_bo) -> u32;
    pub fn gbm_bo_get_height(bo: *mut gbm_bo) -> u32;
}

impl std::fmt::Debug for gbm_bo_handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // SAFETY: every bit pattern is a valid u64.
        write!(f, "gbm_bo_handle(0x{:x})", unsafe { self.u64_ })
    }
}

// --- EGL ------------------------------------------------------------------
pub type EGLDisplay = *mut c_void;
pub type EGLConfig = *mut c_void;
pub type EGLContext = *mut c_void;
pub type EGLSurface = *mut c_void;
pub type EGLint = i32;
pub type EGLenum = c_uint;
pub type EGLBoolean = c_uint;
pub type EGLAttrib = isize;

pub const EGL_TRUE: EGLBoolean = 1;
pub const EGL_NONE: EGLint = 0x3038;
pub const EGL_PLATFORM_GBM_KHR: EGLenum = 0x31D7;
pub const EGL_SUCCESS: EGLint = 0x3000;
pub const EGL_VENDOR: EGLint = 0x3053;
pub const EGL_VERSION: EGLint = 0x3054;
pub const EGL_CLIENT_APIS: EGLint = 0x308D;
pub const EGL_SURFACE_TYPE: EGLint = 0x3033;
pub const EGL_WINDOW_BIT: EGLint = 0x0004;
pub const EGL_RED_SIZE: EGLint = 0x3024;
pub const EGL_GREEN_SIZE: EGLint = 0x3023;
pub const EGL_BLUE_SIZE: EGLint = 0x3022;
pub const EGL_ALPHA_SIZE: EGLint = 0x3021;
pub const EGL_RENDERABLE_TYPE: EGLint = 0x3040;
pub const EGL_OPENGL_ES2_BIT: EGLint = 0x0004;
pub const EGL_NATIVE_VISUAL_ID: EGLint = 0x302E;
pub const EGL_OPENGL_ES_API: EGLenum = 0x30A0;
pub const EGL_CONTEXT_CLIENT_VERSION: EGLint = 0x3098;

#[link(name = "EGL")]
extern "C" {
    pub fn eglGetPlatformDisplay(
        platform: EGLenum,
        native: *mut c_void,
        attribs: *const EGLAttrib,
    ) -> EGLDisplay;
    pub fn eglInitialize(dpy: EGLDisplay, major: *mut EGLint, minor: *mut EGLint) -> EGLBoolean;
    pub fn eglTerminate(dpy: EGLDisplay) -> EGLBoolean;
    pub fn eglQueryString(dpy: EGLDisplay, name: EGLint) -> *const c_char;
    pub fn eglBindAPI(api: EGLenum) -> EGLBoolean;
    pub fn eglGetConfigs(
        dpy: EGLDisplay,
        configs: *mut EGLConfig,
        size: EGLint,
        num: *mut EGLint,
    ) -> EGLBoolean;
    pub fn eglChooseConfig(
        dpy: EGLDisplay,
        attribs: *const EGLint,
        configs: *mut EGLConfig,
        size: EGLint,
        num: *mut EGLint,
    ) -> EGLBoolean;
    pub fn eglGetConfigAttrib(
        dpy: EGLDisplay,
        config: EGLConfig,
        attr: EGLint,
        value: *mut EGLint,
    ) -> EGLBoolean;
    pub fn eglCreateContext(
        dpy: EGLDisplay,
        config: EGLConfig,
        share: EGLContext,
        attribs: *const EGLint,
    ) -> EGLContext;
    pub fn eglDestroyContext(dpy: EGLDisplay, ctx: EGLContext) -> EGLBoolean;
    pub fn eglCreatePlatformWindowSurface(
        dpy: EGLDisplay,
        config: EGLConfig,
        native_window: *mut c_void,
        attribs: *const EGLAttrib,
    ) -> EGLSurface;
    pub fn eglDestroySurface(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;
    pub fn eglMakeCurrent(
        dpy: EGLDisplay,
        draw: EGLSurface,
        read: EGLSurface,
        ctx: EGLContext,
    ) -> EGLBoolean;
    pub fn eglSwapBuffers(dpy: EGLDisplay, surface: EGLSurface) -> EGLBoolean;
    pub fn eglGetError() -> EGLint;
}

// --- OpenGL ES 2.0 --------------------------------------------------------
pub type GLuint = c_uint;
pub type GLint = c_int;
pub type GLenum = c_uint;
pub type GLsizei = c_int;
pub type GLfloat = f32;
pub type GLbitfield = c_uint;
pub type GLboolean = u8;

pub const GL_VENDOR: GLenum = 0x1F00;
pub const GL_RENDERER: GLenum = 0x1F01;
pub const GL_VERSION: GLenum = 0x1F02;
pub const GL_SHADING_LANGUAGE_VERSION: GLenum = 0x8B8C;
pub const GL_VERTEX_SHADER: GLenum = 0x8B31;
pub const GL_FRAGMENT_SHADER: GLenum = 0x8B30;
pub const GL_COMPILE_STATUS: GLenum = 0x8B81;
pub const GL_LINK_STATUS: GLenum = 0x8B82;
pub const GL_INFO_LOG_LENGTH: GLenum = 0x8B84;
pub const GL_COLOR_BUFFER_BIT: GLbitfield = 0x4000;
pub const GL_TRIANGLES: GLenum = 0x0004;
pub const GL_FLOAT: GLenum = 0x1406;
pub const GL_FALSE: GLboolean = 0;
pub const GL_TEXTURE_2D: GLenum = 0x0DE1;
pub const GL_TEXTURE0: GLenum = 0x84C0;
pub const GL_TEXTURE_MIN_FILTER: GLenum = 0x2801;
pub const GL_TEXTURE_MAG_FILTER: GLenum = 0x2800;
pub const GL_TEXTURE_WRAP_S: GLenum = 0x2802;
pub const GL_TEXTURE_WRAP_T: GLenum = 0x2803;
pub const GL_NEAREST: GLint = 0x2600;
pub const GL_CLAMP_TO_EDGE: GLint = 0x812F;
pub const GL_RGBA: GLenum = 0x1908;
pub const GL_UNSIGNED_BYTE: GLenum = 0x1401;
pub const GL_UNPACK_ALIGNMENT: GLenum = 0x0CF5;
pub const GL_BLEND: GLenum = 0x0BE2;
pub const GL_ONE: GLenum = 1;
pub const GL_ONE_MINUS_SRC_ALPHA: GLenum = 0x0303;
pub const GL_TRIANGLE_STRIP: GLenum = 0x0005;
pub const GL_NO_ERROR: GLenum = 0;

#[link(name = "GLESv2")]
extern "C" {
    pub fn glGetString(name: GLenum) -> *const u8;
    pub fn glGetError() -> GLenum;
    pub fn glViewport(x: GLint, y: GLint, w: GLsizei, h: GLsizei);
    pub fn glClearColor(r: GLfloat, g: GLfloat, b: GLfloat, a: GLfloat);
    pub fn glClear(mask: GLbitfield);
    pub fn glCreateShader(kind: GLenum) -> GLuint;
    pub fn glShaderSource(
        shader: GLuint,
        count: GLsizei,
        src: *const *const c_char,
        len: *const GLint,
    );
    pub fn glCompileShader(shader: GLuint);
    pub fn glGetShaderiv(shader: GLuint, pname: GLenum, out: *mut GLint);
    pub fn glGetShaderInfoLog(shader: GLuint, size: GLsizei, len: *mut GLsizei, log: *mut c_char);
    pub fn glDeleteShader(shader: GLuint);
    pub fn glCreateProgram() -> GLuint;
    pub fn glAttachShader(program: GLuint, shader: GLuint);
    pub fn glBindAttribLocation(program: GLuint, index: GLuint, name: *const c_char);
    pub fn glLinkProgram(program: GLuint);
    pub fn glGetProgramiv(program: GLuint, pname: GLenum, out: *mut GLint);
    pub fn glGetProgramInfoLog(program: GLuint, size: GLsizei, len: *mut GLsizei, log: *mut c_char);
    pub fn glUseProgram(program: GLuint);
    pub fn glDeleteProgram(program: GLuint);
    pub fn glGetUniformLocation(program: GLuint, name: *const c_char) -> GLint;
    pub fn glUniform1f(loc: GLint, v: GLfloat);
    pub fn glUniform2f(loc: GLint, x: GLfloat, y: GLfloat);
    pub fn glEnableVertexAttribArray(index: GLuint);
    pub fn glVertexAttribPointer(
        index: GLuint,
        size: GLint,
        kind: GLenum,
        normalized: GLboolean,
        stride: GLsizei,
        ptr: *const c_void,
    );
    pub fn glDrawArrays(mode: GLenum, first: GLint, count: GLsizei);
    pub fn glFinish();
    pub fn glGenTextures(n: GLsizei, textures: *mut GLuint);
    pub fn glDeleteTextures(n: GLsizei, textures: *const GLuint);
    pub fn glBindTexture(target: GLenum, texture: GLuint);
    pub fn glActiveTexture(texture: GLenum);
    pub fn glTexParameteri(target: GLenum, pname: GLenum, param: GLint);
    pub fn glPixelStorei(pname: GLenum, param: GLint);
    pub fn glTexImage2D(
        target: GLenum,
        level: GLint,
        internalformat: GLint,
        width: GLsizei,
        height: GLsizei,
        border: GLint,
        format: GLenum,
        typ: GLenum,
        pixels: *const c_void,
    );
    pub fn glUniform1i(location: GLint, v0: GLint);
    pub fn glUniform4f(location: GLint, v0: GLfloat, v1: GLfloat, v2: GLfloat, v3: GLfloat);
    pub fn glEnable(cap: GLenum);
    pub fn glDisable(cap: GLenum);
    pub fn glBlendFunc(sfactor: GLenum, dfactor: GLenum);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xrgb8888_fourcc_matches_drm_fourcc_h() {
        // drm_fourcc.h: #define DRM_FORMAT_XRGB8888 fourcc_code('X', 'R', '2', '4') = 0x34325258
        assert_eq!(GBM_FORMAT_XRGB8888, 0x3432_5258);
    }

    #[test]
    fn gbm_bo_handle_is_64_bits() {
        assert_eq!(std::mem::size_of::<gbm_bo_handle>(), 8);
    }
}
