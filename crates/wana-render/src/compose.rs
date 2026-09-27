//! Compositing primitives (Phase 10): textures made from client pixels and
//! textured quads placed in output pixel coordinates.
//!
//! Pixels arrive as Wayland shm data: ARGB8888 / XRGB8888 little-endian,
//! i.e. B, G, R, A bytes in memory, rows packed (the caller removes the
//! client's stride). They are uploaded as GL_RGBA bytes and swizzled back in
//! the shader (`.bgra`), so no BGRA texture extension is needed. Wayland
//! ARGB is premultiplied alpha; blending is `ONE, ONE_MINUS_SRC_ALPHA`.
//! XRGB (and any surface drawn opaque) has its alpha forced to 1.

use crate::ffi::*;
use crate::gl;

/// A GL texture holding one client buffer's pixels.
#[derive(Debug)]
pub struct Texture {
    id: GLuint,
    pub width: u32,
    pub height: u32,
    /// True for XRGB: the fourth byte is undefined and must be ignored.
    pub opaque: bool,
}

impl Texture {
    /// Uploads packed BGRA bytes (`width * height * 4` of them).
    pub fn upload(bgra: &[u8], width: u32, height: u32, opaque: bool) -> Result<Texture, String> {
        let expected = width as usize * height as usize * 4;
        if bgra.len() != expected || width == 0 || height == 0 {
            return Err(format!(
                "texture {width}x{height}: {} bytes, expected {expected}",
                bgra.len()
            ));
        }
        let mut id = 0;
        // SAFETY: current context; `bgra` holds exactly width*height*4 bytes
        // and glTexImage2D copies them before returning.
        unsafe {
            glGenTextures(1, &mut id);
            glBindTexture(GL_TEXTURE_2D, id);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
            glPixelStorei(GL_UNPACK_ALIGNMENT, 4);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA as GLint,
                width as GLsizei,
                height as GLsizei,
                0,
                GL_RGBA,
                GL_UNSIGNED_BYTE,
                bgra.as_ptr().cast(),
            );
        }
        let tex = Texture {
            id,
            width,
            height,
            opaque,
        };
        gl::check("glTexImage2D")?;
        Ok(tex)
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        // SAFETY: the texture belongs to the current context.
        unsafe { glDeleteTextures(1, &self.id) };
    }
}

const VERTEX: &str = "#version 100
attribute vec2 a_pos;
uniform vec4 u_rect;   // x, y, width, height in output pixels (top-left origin)
uniform vec2 u_screen; // output size in pixels
varying vec2 v_uv;
void main() {
    vec2 px = u_rect.xy + a_pos * u_rect.zw;
    gl_Position = vec4(px.x / u_screen.x * 2.0 - 1.0, 1.0 - px.y / u_screen.y * 2.0, 0.0, 1.0);
    v_uv = a_pos;
}
";

const FRAGMENT: &str = "#version 100
precision mediump float;
uniform sampler2D u_tex;
uniform float u_opaque;
varying vec2 v_uv;
void main() {
    vec4 c = texture2D(u_tex, v_uv).bgra;
    gl_FragColor = u_opaque > 0.5 ? vec4(c.rgb, 1.0) : c;
}
";

/// Unit quad as a triangle strip (0,0) (1,0) (0,1) (1,1).
static QUAD: [f32; 8] = [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// Draws a background color and textured rectangles on an output.
#[derive(Debug)]
pub struct Composer {
    program: GLuint,
    u_rect: GLint,
    u_screen: GLint,
    u_tex: GLint,
    u_opaque: GLint,
    width: u32,
    height: u32,
}

/// 0xRRGGBB to GL color components.
fn rgb(color: u32) -> (f32, f32, f32) {
    let c = |shift: u32| ((color >> shift) & 0xFF) as f32 / 255.0;
    (c(16), c(8), c(0))
}

impl Composer {
    pub fn new(width: u32, height: u32) -> Result<Composer, String> {
        let program = gl::program(VERTEX, FRAGMENT)?;
        Ok(Composer {
            program,
            u_rect: gl::uniform(program, c"u_rect"),
            u_screen: gl::uniform(program, c"u_screen"),
            u_tex: gl::uniform(program, c"u_tex"),
            u_opaque: gl::uniform(program, c"u_opaque"),
            width,
            height,
        })
    }

    /// Starts a frame: viewport and background color.
    pub fn begin(&self, background: u32) {
        let (r, g, b) = rgb(background);
        // SAFETY: current context.
        unsafe {
            glViewport(0, 0, self.width as GLsizei, self.height as GLsizei);
            glClearColor(r, g, b, 1.0);
            glClear(GL_COLOR_BUFFER_BIT);
        }
    }

    /// Fills a solid output-space rectangle. Coordinates use the same
    /// top-left origin as textured surfaces; OpenGL's scissor rectangle is
    /// converted to its bottom-left origin here.
    pub fn draw_solid(
        &self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        color: u32,
    ) -> Result<(), String> {
        if width <= 0 || height <= 0 {
            return Ok(());
        }
        let x0 = x.clamp(0, self.width as i32);
        let y0 = y.clamp(0, self.height as i32);
        let x1 = (x + width).clamp(0, self.width as i32);
        let y1 = (y + height).clamp(0, self.height as i32);
        let w = x1 - x0;
        let h = y1 - y0;
        if w <= 0 || h <= 0 {
            return Ok(());
        }
        let (r, g, b) = rgb(color);
        let gl_y = self.height as i32 - y1;
        // SAFETY: current context. Scissor coordinates were clamped to the
        // framebuffer; glClear affects only that rectangle while the test is
        // enabled.
        unsafe {
            glEnable(GL_SCISSOR_TEST);
            glScissor(x0, gl_y, w, h);
            glClearColor(r, g, b, 1.0);
            glClear(GL_COLOR_BUFFER_BIT);
            glDisable(GL_SCISSOR_TEST);
        }
        gl::check("draw solid")
    }

    /// Draws `tex` with its top-left corner at (x, y), at its own size.
    pub fn draw(&self, tex: &Texture, x: i32, y: i32) -> Result<(), String> {
        // SAFETY: current context; QUAD is 'static, so the client-side
        // attribute pointer stays valid through glDrawArrays.
        unsafe {
            glUseProgram(self.program);
            glActiveTexture(GL_TEXTURE0);
            glBindTexture(GL_TEXTURE_2D, tex.id);
            glUniform1i(self.u_tex, 0);
            glUniform2f(self.u_screen, self.width as f32, self.height as f32);
            glUniform4f(
                self.u_rect,
                x as f32,
                y as f32,
                tex.width as f32,
                tex.height as f32,
            );
            glUniform1f(self.u_opaque, if tex.opaque { 1.0 } else { 0.0 });
            if tex.opaque {
                glDisable(GL_BLEND);
            } else {
                glEnable(GL_BLEND);
                glBlendFunc(GL_ONE, GL_ONE_MINUS_SRC_ALPHA);
            }
            glEnableVertexAttribArray(0);
            glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, QUAD.as_ptr().cast());
            glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
        }
        gl::check("draw texture")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_split_into_components() {
        let (r, g, b) = rgb(0x4F8CFF);
        assert_eq!(
            (
                (r * 255.0).round(),
                (g * 255.0).round(),
                (b * 255.0).round()
            ),
            (79.0, 140.0, 255.0)
        );
    }

    #[test]
    fn top_left_scissor_conversion_is_bounded_by_design() {
        // The implementation converts y to height-(y+h). Keep the source
        // contract explicit so future refactors do not silently flip
        // compositor-owned decorations.
        assert!(include_str!("compose.rs").contains("let gl_y = self.height as i32 - y1;"));
    }

    #[test]
    fn shaders_swizzle_and_place_in_pixels() {
        assert!(FRAGMENT.contains(".bgra"), "shm is BGRA in memory");
        assert!(
            VERTEX.contains("1.0 - px.y / u_screen.y * 2.0"),
            "top-left origin"
        );
    }
}
