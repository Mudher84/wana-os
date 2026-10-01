//! The Phase 8 test scene, drawn entirely on the GPU with one fragment
//! shader: background, centered rounded accent square (signed distance
//! field, anti-aliased edge), 8 px border and an animated progress bar.
//! Colors match the wana-kms CPU pattern, so the same screenshot pixel
//! checks apply to both paths.

use crate::ffi::*;
use crate::gl;

/// Colors as 0xRRGGBB (shared with the CPU pattern in wana-kms).
pub const BACKGROUND: u32 = 0x16213E;
pub const ACCENT: u32 = 0x4F8CFF;
pub const BORDER: u32 = 0xFFFFFF;

/// "0.086275, 0.129412, 0.243137" for a GLSL vec3 literal.
pub fn glsl_rgb(color: u32) -> String {
    let c = |shift: u32| f64::from((color >> shift) & 0xFF) / 255.0;
    format!("{:.6}, {:.6}, {:.6}", c(16), c(8), c(0))
}

const VERTEX: &str = "#version 100
attribute vec2 a_pos;
void main() { gl_Position = vec4(a_pos, 0.0, 1.0); }
";

fn fragment_source() -> String {
    format!(
        "#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
uniform vec2 u_res;
uniform float u_progress;
const vec3 BG = vec3({bg});
const vec3 ACCENT = vec3({accent});
const vec3 BORDER = vec3({border});
void main() {{
    vec2 p = gl_FragCoord.xy;
    if (p.x < 8.0 || p.y < 8.0 || p.x > u_res.x - 8.0 || p.y > u_res.y - 8.0) {{
        gl_FragColor = vec4(BORDER, 1.0);
        return;
    }}
    vec3 col = BG;
    // Rounded square, side = height/3, radius 18% of the side.
    float half_side = u_res.y / 6.0;
    float r = half_side * 0.36;
    vec2 q = abs(p - u_res * 0.5) - vec2(half_side - r);
    float d = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
    col = mix(col, ACCENT, clamp(0.5 - d, 0.0, 1.0));
    // Progress bar near the bottom (proves frames are re-rendered).
    float bar_y = u_res.y * 0.1;
    if (abs(p.y - bar_y) < 4.0 && p.x > u_res.x * 0.25 && p.x < u_res.x * (0.25 + 0.5 * u_progress)) {{
        col = mix(ACCENT, BORDER, 0.35);
    }}
    gl_FragColor = vec4(col, 1.0);
}}
",
        bg = glsl_rgb(BACKGROUND),
        accent = glsl_rgb(ACCENT),
        border = glsl_rgb(BORDER),
    )
}

/// Full-screen triangle covering the viewport (client-side array, ES2).
static TRIANGLE: [f32; 6] = [-1.0, -1.0, 3.0, -1.0, -1.0, 3.0];

#[derive(Debug)]
pub struct Scene {
    program: GLuint,
    u_res: GLint,
    u_progress: GLint,
    width: u32,
    height: u32,
}

impl Scene {
    pub fn new(width: u32, height: u32) -> Result<Scene, String> {
        let program = gl::program(VERTEX, &fragment_source())?;
        Ok(Scene {
            program,
            u_res: gl::uniform(program, c"u_res"),
            u_progress: gl::uniform(program, c"u_progress"),
            width,
            height,
        })
    }

    /// Renders one frame; `progress` in 0..=1 drives the progress bar.
    pub fn draw(&self, progress: f32) -> Result<(), String> {
        // SAFETY: current context; TRIANGLE is 'static, so the client-side
        // attribute pointer stays valid through glDrawArrays.
        unsafe {
            glViewport(0, 0, self.width as GLsizei, self.height as GLsizei);
            glUseProgram(self.program);
            glUniform2f(self.u_res, self.width as f32, self.height as f32);
            glUniform1f(self.u_progress, progress);
            glEnableVertexAttribArray(0);
            glVertexAttribPointer(0, 2, GL_FLOAT, GL_FALSE, 0, TRIANGLE.as_ptr().cast());
            glDrawArrays(GL_TRIANGLES, 0, 3);
        }
        gl::check("draw")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glsl_colors_round_trip_to_8_bit() {
        for color in [BACKGROUND, ACCENT, BORDER] {
            let parts: Vec<f64> = glsl_rgb(color)
                .split(", ")
                .map(|s| s.parse().unwrap())
                .collect();
            let back = parts
                .iter()
                .fold(0u32, |acc, c| (acc << 8) | (c * 255.0).round() as u32);
            assert_eq!(back, color);
        }
    }

    #[test]
    fn fragment_shader_embeds_the_palette() {
        let src = fragment_source();
        assert!(src.contains("const vec3 BG = vec3(0.086275, 0.129412, 0.243137);"));
        assert!(src.contains("uniform float u_progress;"));
    }
}
