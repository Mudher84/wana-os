//! Small OpenGL ES 2 helpers: strings, shader programs, errors.

use crate::ffi::*;
use std::ffi::{CStr, CString};

/// glGetString as a Rust string.
pub fn string(name: GLenum) -> String {
    // SAFETY: a context is current (callers create Egl first).
    let p = unsafe { glGetString(name) };
    if p.is_null() {
        return "?".into();
    }
    // SAFETY: non-null NUL-terminated string owned by the GL driver.
    unsafe { CStr::from_ptr(p.cast()) }
        .to_string_lossy()
        .into_owned()
}

pub fn check(what: &str) -> Result<(), String> {
    // SAFETY: current context.
    let err = unsafe { glGetError() };
    if err == GL_NO_ERROR {
        Ok(())
    } else {
        Err(format!("{what}: GL error 0x{err:04x}"))
    }
}

fn compile(kind: GLenum, src: &str) -> Result<GLuint, String> {
    let c_src = CString::new(src).map_err(|e| e.to_string())?;
    // SAFETY: current context; source pointer valid for the call.
    unsafe {
        let shader = glCreateShader(kind);
        glShaderSource(shader, 1, &c_src.as_ptr(), std::ptr::null());
        glCompileShader(shader);
        let mut ok = 0;
        glGetShaderiv(shader, GL_COMPILE_STATUS, &mut ok);
        if ok == 0 {
            let log = info_log(shader, glGetShaderiv, glGetShaderInfoLog);
            glDeleteShader(shader);
            let stage = if kind == GL_VERTEX_SHADER {
                "vertex"
            } else {
                "fragment"
            };
            return Err(format!("{stage} shader: {log}"));
        }
        Ok(shader)
    }
}

type GetIv = unsafe extern "C" fn(GLuint, GLenum, *mut GLint);
type GetLog = unsafe extern "C" fn(GLuint, GLsizei, *mut GLsizei, *mut std::os::raw::c_char);

unsafe fn info_log(obj: GLuint, get_iv: GetIv, get_log: GetLog) -> String {
    let mut len = 0;
    // SAFETY: obj is a valid shader/program for the matching functions.
    unsafe { get_iv(obj, GL_INFO_LOG_LENGTH, &mut len) };
    let mut buf = vec![0u8; len.max(1) as usize];
    // SAFETY: buf has room for len bytes.
    unsafe { get_log(obj, len, std::ptr::null_mut(), buf.as_mut_ptr().cast()) };
    String::from_utf8_lossy(&buf)
        .trim_end_matches('\0')
        .trim()
        .to_owned()
}

/// Compiles and links a program; attribute 0 is bound to `a_pos`.
pub fn program(vertex: &str, fragment: &str) -> Result<GLuint, String> {
    let vs = compile(GL_VERTEX_SHADER, vertex)?;
    let fs = compile(GL_FRAGMENT_SHADER, fragment)?;
    // SAFETY: current context; shader ids valid; attribute name NUL-terminated.
    unsafe {
        let prog = glCreateProgram();
        glAttachShader(prog, vs);
        glAttachShader(prog, fs);
        glBindAttribLocation(prog, 0, c"a_pos".as_ptr());
        glLinkProgram(prog);
        glDeleteShader(vs);
        glDeleteShader(fs);
        let mut ok = 0;
        glGetProgramiv(prog, GL_LINK_STATUS, &mut ok);
        if ok == 0 {
            let log = info_log(prog, glGetProgramiv, glGetProgramInfoLog);
            glDeleteProgram(prog);
            return Err(format!("link: {log}"));
        }
        Ok(prog)
    }
}

pub fn uniform(prog: GLuint, name: &std::ffi::CStr) -> GLint {
    // SAFETY: valid program; NUL-terminated name.
    unsafe { glGetUniformLocation(prog, name.as_ptr()) }
}
