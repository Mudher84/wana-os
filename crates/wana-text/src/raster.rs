//! Drawing text: glyph outlines from HarfBuzz (`hb_font_draw_glyph`),
//! rasterized here into antialiased coverage masks, blended into pixels.
//!
//! The rasterizer is signed-area accumulation (the technique of libart and
//! font-rs): each outline edge adds, per pixel it crosses, the signed area
//! it covers; a running sum along each row then gives the coverage. Curves
//! are flattened to lines within 1/8 px. Overlapping contours of variable
//! fonts add up and are clamped (non-zero fill). Everything is plain `f32`
//! arithmetic in a fixed order, so a mask is bit-identical on every machine;
//! a rendered line can be checked by its hash.

use crate::ffi;
use crate::font::Font;
use crate::layout::{FontSet, Layout};
use std::ffi::c_void;

/// Flattening tolerance in pixels.
const TOLERANCE: f32 = 0.125;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f32,
    y: f32,
}

/// An outline flattened to line segments, in pixels (y down).
#[derive(Debug, Default)]
pub struct Path {
    lines: Vec<(Point, Point)>,
    start: Option<Point>,
    cur: Option<Point>,
    // Font units -> pixels.
    scale: f32,
    ox: f32,
    oy: f32,
}

impl Path {
    fn map(&self, x: f32, y: f32) -> Point {
        Point {
            x: self.ox + x * self.scale,
            y: self.oy - y * self.scale,
        }
    }

    fn move_to(&mut self, p: Point) {
        self.close();
        self.start = Some(p);
        self.cur = Some(p);
    }

    fn line_to(&mut self, p: Point) {
        if let Some(c) = self.cur {
            self.lines.push((c, p));
        }
        self.cur = Some(p);
    }

    fn quad_to(&mut self, c: Point, p: Point) {
        let Some(p0) = self.cur else { return };
        // Segments so that the deviation stays within TOLERANCE.
        let dd = ((p0.x - 2.0 * c.x + p.x).powi(2) + (p0.y - 2.0 * c.y + p.y).powi(2)).sqrt();
        let n = ((dd / (8.0 * TOLERANCE)).sqrt().ceil() as usize).clamp(1, 64);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            self.line_to(Point {
                x: u * u * p0.x + 2.0 * u * t * c.x + t * t * p.x,
                y: u * u * p0.y + 2.0 * u * t * c.y + t * t * p.y,
            });
        }
    }

    fn cubic_to(&mut self, c1: Point, c2: Point, p: Point) {
        let Some(p0) = self.cur else { return };
        let d1 = ((p0.x - 2.0 * c1.x + c2.x).powi(2) + (p0.y - 2.0 * c1.y + c2.y).powi(2)).sqrt();
        let d2 = ((c1.x - 2.0 * c2.x + p.x).powi(2) + (c1.y - 2.0 * c2.y + p.y).powi(2)).sqrt();
        let dd = d1.max(d2);
        let n = ((dd * 0.75 / TOLERANCE).sqrt().ceil() as usize).clamp(1, 64);
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.line_to(Point {
                x: a * p0.x + b * c1.x + c * c2.x + d * p.x,
                y: a * p0.y + b * c1.y + c * c2.y + d * p.y,
            });
        }
    }

    fn close(&mut self) {
        if let (Some(c), Some(s)) = (self.cur, self.start) {
            if c != s {
                self.lines.push((c, s));
            }
        }
        self.cur = self.start;
    }
}

// HarfBuzz draw callbacks: draw_data is the `Path` being built.
unsafe extern "C" fn cb_move(
    _: *mut ffi::hb_draw_funcs_t,
    data: *mut c_void,
    _: *mut ffi::hb_draw_state_t,
    x: f32,
    y: f32,
    _: *mut c_void,
) {
    // SAFETY: data is the &mut Path passed to hb_font_draw_glyph.
    let p = unsafe { &mut *(data as *mut Path) };
    let pt = p.map(x, y);
    p.move_to(pt);
}

unsafe extern "C" fn cb_line(
    _: *mut ffi::hb_draw_funcs_t,
    data: *mut c_void,
    _: *mut ffi::hb_draw_state_t,
    x: f32,
    y: f32,
    _: *mut c_void,
) {
    // SAFETY: as above.
    let p = unsafe { &mut *(data as *mut Path) };
    let pt = p.map(x, y);
    p.line_to(pt);
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn cb_quad(
    _: *mut ffi::hb_draw_funcs_t,
    data: *mut c_void,
    _: *mut ffi::hb_draw_state_t,
    cx: f32,
    cy: f32,
    x: f32,
    y: f32,
    _: *mut c_void,
) {
    // SAFETY: as above.
    let p = unsafe { &mut *(data as *mut Path) };
    let (c, pt) = (p.map(cx, cy), p.map(x, y));
    p.quad_to(c, pt);
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn cb_cubic(
    _: *mut ffi::hb_draw_funcs_t,
    data: *mut c_void,
    _: *mut ffi::hb_draw_state_t,
    c1x: f32,
    c1y: f32,
    c2x: f32,
    c2y: f32,
    x: f32,
    y: f32,
    _: *mut c_void,
) {
    // SAFETY: as above.
    let p = unsafe { &mut *(data as *mut Path) };
    let (c1, c2, pt) = (p.map(c1x, c1y), p.map(c2x, c2y), p.map(x, y));
    p.cubic_to(c1, c2, pt);
}

unsafe extern "C" fn cb_close(
    _: *mut ffi::hb_draw_funcs_t,
    data: *mut c_void,
    _: *mut ffi::hb_draw_state_t,
    _: *mut c_void,
) {
    // SAFETY: as above.
    let p = unsafe { &mut *(data as *mut Path) };
    p.close();
}

/// The glyph outline of `glyph` at `size` px with its origin (pen position
/// on the baseline) at (`x`, `y`).
pub fn outline(font: &Font, glyph: u32, size: f32, x: f32, y: f32) -> Path {
    let mut path = Path {
        scale: size / font.units_per_em() as f32,
        ox: x,
        oy: y,
        ..Default::default()
    };
    // SAFETY: the funcs object is created, filled with callbacks whose
    // signatures match hb-draw.h, used for one synchronous draw call with
    // `path` as draw_data (alive and exclusively borrowed for the call),
    // then destroyed. NULL destroy callbacks and user data are allowed.
    unsafe {
        let f = ffi::hb_draw_funcs_create();
        let null = std::ptr::null_mut();
        ffi::hb_draw_funcs_set_move_to_func(f, cb_move, null, std::ptr::null());
        ffi::hb_draw_funcs_set_line_to_func(f, cb_line, null, std::ptr::null());
        ffi::hb_draw_funcs_set_quadratic_to_func(f, cb_quad, null, std::ptr::null());
        ffi::hb_draw_funcs_set_cubic_to_func(f, cb_cubic, null, std::ptr::null());
        ffi::hb_draw_funcs_set_close_path_func(f, cb_close, null, std::ptr::null());
        ffi::hb_draw_funcs_make_immutable(f);
        ffi::hb_font_draw_glyph(font.raw(), glyph, f, (&mut path as *mut Path).cast());
        ffi::hb_draw_funcs_destroy(f);
    }
    path.close();
    path
}

/// A coverage mask: `alpha` is `width` x `height`, its top-left pixel at
/// (`left`, `top`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mask {
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

impl Mask {
    /// Sum of coverage in pixels (255 = one full pixel).
    pub fn area(&self) -> f32 {
        self.alpha.iter().map(|&a| a as f32).sum::<f32>() / 255.0
    }
}

/// Rasterizes a flattened path into a coverage mask.
pub fn rasterize(path: &Path) -> Mask {
    if path.lines.is_empty() {
        return Mask {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            alpha: Vec::new(),
        };
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (a, b) in &path.lines {
        for p in [a, b] {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
    }
    let (left, top) = (x0.floor() as i32, y0.floor() as i32);
    let w = (x1.ceil() as i32 - left).max(1) as usize;
    let h = (y1.ceil() as i32 - top).max(1) as usize;
    // One extra cell per row absorbs the area right of the last pixel.
    let stride = w + 2;
    let mut acc = vec![0f32; stride * h];
    for (a, b) in &path.lines {
        let pa = Point {
            x: a.x - left as f32,
            y: a.y - top as f32,
        };
        let pb = Point {
            x: b.x - left as f32,
            y: b.y - top as f32,
        };
        edge(&mut acc, stride, h, pa, pb);
    }
    let mut alpha = vec![0u8; w * h];
    for row in 0..h {
        let mut sum = 0f32;
        for col in 0..w {
            sum += acc[row * stride + col];
            alpha[row * w + col] = (sum.abs().min(1.0) * 255.0 + 0.5) as u8;
        }
    }
    Mask {
        left,
        top,
        width: w as u32,
        height: h as u32,
        alpha,
    }
}

/// Adds the signed area of one edge. `acc` rows are `stride` cells wide;
/// the edge lies within [0, stride - 2] x [0, h].
fn edge(acc: &mut [f32], stride: usize, h: usize, p0: Point, p1: Point) {
    if p0.y == p1.y {
        return;
    }
    let (dir, p0, p1) = if p0.y < p1.y {
        (1.0f32, p0, p1)
    } else {
        (-1.0, p1, p0)
    };
    let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
    let mut x = p0.x;
    let y_start = p0.y.max(0.0) as usize;
    let y_end = (p1.y.ceil() as usize).min(h);
    for y in y_start..y_end {
        let row = y * stride;
        let dy = ((y + 1) as f32).min(p1.y) - (y as f32).max(p0.y);
        let xnext = x + dxdy * dy;
        let d = dy * dir;
        let (xa, xb) = if x < xnext { (x, xnext) } else { (xnext, x) };
        let xa_floor = xa.floor();
        let xai = xa_floor as usize;
        let xbi = xb.ceil() as usize;
        if xbi <= xai + 1 {
            // Within one pixel column: split by the mean x.
            let xm = 0.5 * (x + xnext) - xa_floor;
            acc[row + xai] += d - d * xm;
            acc[row + xai + 1] += d * xm;
        } else {
            let s = 1.0 / (xb - xa);
            let fa = xa - xa_floor;
            let a0 = 0.5 * s * (1.0 - fa) * (1.0 - fa);
            let fb = xb - xb.ceil() + 1.0;
            let am = 0.5 * s * fb * fb;
            acc[row + xai] += d * a0;
            if xbi == xai + 2 {
                acc[row + xai + 1] += d * (1.0 - a0 - am);
            } else {
                let a1 = s * (1.5 - fa);
                acc[row + xai + 1] += d * (a1 - a0);
                for xi in xai + 2..xbi - 1 {
                    acc[row + xi] += d * s;
                }
                let a2 = a1 + (xbi - xai - 3) as f32 * s;
                acc[row + xbi - 1] += d * (1.0 - a2 - am);
            }
            acc[row + xbi] += d * am;
        }
        x = xnext;
    }
}

/// An XRGB8888 pixel buffer (the wl_shm format), row-major, no padding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, rgb: u32) -> Canvas {
        Canvas {
            width,
            height,
            pixels: vec![0xFF00_0000 | rgb; (width * height) as usize],
        }
    }

    /// Blends `rgb` through `mask` (integer math, exact and repeatable).
    pub fn blend(&mut self, mask: &Mask, rgb: u32) {
        for my in 0..mask.height as i32 {
            let y = mask.top + my;
            if y < 0 || y >= self.height as i32 {
                continue;
            }
            for mx in 0..mask.width as i32 {
                let x = mask.left + mx;
                if x < 0 || x >= self.width as i32 {
                    continue;
                }
                let a = mask.alpha[(my as u32 * mask.width + mx as u32) as usize] as u32;
                if a == 0 {
                    continue;
                }
                let i = (y as u32 * self.width + x as u32) as usize;
                let d = self.pixels[i];
                let mix = |shift: u32| {
                    let s = (rgb >> shift) & 0xFF;
                    let t = (d >> shift) & 0xFF;
                    ((s * a + t * (255 - a) + 127) / 255) << shift
                };
                self.pixels[i] = 0xFF00_0000 | mix(16) | mix(8) | mix(0);
            }
        }
    }

    /// Little-endian bytes, as in a wl_shm XRGB8888 buffer.
    pub fn bytes(&self) -> Vec<u8> {
        self.pixels.iter().flat_map(|p| p.to_le_bytes()).collect()
    }
}

/// Draws every glyph of `layout` with its top-left at (`x`, `y`).
pub fn draw(
    canvas: &mut Canvas,
    layout: &Layout,
    fonts: &FontSet,
    size: f32,
    x: f32,
    y: f32,
    rgb: u32,
) {
    for line in &layout.lines {
        for g in &line.glyphs {
            let path = outline(
                &fonts.fonts[g.font],
                g.id,
                size,
                x + g.x,
                y + line.baseline + g.y,
            );
            canvas.blend(&rasterize(&path), rgb);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::test_fonts_dir;

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Path {
        let mut p = Path {
            scale: 1.0,
            ..Default::default()
        };
        p.move_to(Point { x: x0, y: y0 });
        p.line_to(Point { x: x1, y: y0 });
        p.line_to(Point { x: x1, y: y1 });
        p.line_to(Point { x: x0, y: y1 });
        p.close();
        p
    }

    #[test]
    fn pixel_aligned_square_is_solid() {
        let m = rasterize(&rect(2.0, 3.0, 6.0, 5.0));
        assert_eq!((m.left, m.top, m.width, m.height), (2, 3, 4, 2));
        assert!(m.alpha.iter().all(|&a| a == 255), "{:?}", m.alpha);
    }

    #[test]
    fn half_pixel_edges_are_half_covered() {
        let m = rasterize(&rect(0.5, 0.0, 3.5, 1.0));
        assert_eq!(m.alpha, [128, 255, 255, 128]);
        assert!((m.area() - 3.0).abs() < 0.01);
    }

    #[test]
    fn diagonal_edge_covers_the_right_area() {
        // A right triangle with legs 8: area 32 pixels.
        let mut p = Path {
            scale: 1.0,
            ..Default::default()
        };
        p.move_to(Point { x: 0.0, y: 0.0 });
        p.line_to(Point { x: 8.0, y: 8.0 });
        p.line_to(Point { x: 0.0, y: 8.0 });
        p.close();
        let m = rasterize(&p);
        assert!((m.area() - 32.0).abs() < 0.05, "{}", m.area());
        // Pixels on the diagonal are half covered.
        assert_eq!(m.alpha[3 * 8 + 3], 128);
    }

    #[test]
    fn winding_direction_does_not_matter_and_overlaps_clamp() {
        let a = rasterize(&rect(0.0, 0.0, 4.0, 4.0));
        let b = rasterize(&rect(4.0, 4.0, 0.0, 0.0));
        assert_eq!(a.alpha, b.alpha);
        // Two overlapping squares (as variable fonts overlap contours).
        let mut p = rect(0.0, 0.0, 4.0, 4.0);
        let q = rect(2.0, 0.0, 6.0, 4.0);
        p.lines.extend(q.lines);
        let m = rasterize(&p);
        assert!(m.alpha.iter().all(|&a| a == 255), "non-zero fill");
    }

    #[test]
    fn glyph_masks_match_harfbuzz_extents() {
        let dir = test_fonts_dir();
        let f = Font::load(&dir.join("NotoSans-VF.ttf")).unwrap();
        let size = 32.0;
        let s = size / f.units_per_em() as f32;
        for c in ['W', 'a', 'g', 'O'] {
            let g = f.glyph(c).unwrap();
            let mut e = ffi::hb_glyph_extents_t::default();
            // SAFETY: valid font; e is an out pointer.
            unsafe { ffi::hb_font_get_glyph_extents(f.raw(), g, &mut e) };
            let m = rasterize(&outline(&f, g, size, 10.0, 40.0));
            // The mask's box is the extents' box rounded outwards.
            let (l, t) = (10.0 + e.x_bearing as f32 * s, 40.0 - e.y_bearing as f32 * s);
            let (r, b) = (l + e.width as f32 * s, t - e.height as f32 * s);
            assert_eq!(m.left, l.floor() as i32, "{c} left");
            assert_eq!(m.top, t.floor() as i32, "{c} top");
            assert_eq!(m.left + m.width as i32, r.ceil() as i32, "{c} right");
            assert_eq!(m.top + m.height as i32, b.ceil() as i32, "{c} bottom");
            // Ink but not a filled box.
            assert!(
                m.area() > 10.0 && m.area() < (m.width * m.height) as f32 * 0.9,
                "{c}"
            );
        }
        // 'O' has a hole: its center is empty.
        let m = rasterize(&outline(&f, f.glyph('O').unwrap(), 64.0, 0.0, 64.0));
        let center = m.alpha[(m.height / 2 * m.width + m.width / 2) as usize];
        assert_eq!(center, 0, "counter of O");
    }

    #[test]
    fn blending_is_exact() {
        let mut c = Canvas::new(2, 1, 0xFFFFFF);
        let m = Mask {
            left: 0,
            top: 0,
            width: 2,
            height: 1,
            alpha: vec![255, 128],
        };
        c.blend(&m, 0x000000);
        assert_eq!(c.pixels, [0xFF00_0000, 0xFF7F_7F7F]);
        assert_eq!(&c.bytes()[..4], &[0, 0, 0, 0xFF]);
    }
}
