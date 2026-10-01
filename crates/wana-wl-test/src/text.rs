//! `--text`: a window showing Arabic and Latin text drawn by wana-text
//! (pinned fonts, shaping, BiDi, layout, rasterizer): the whole text stack
//! reaching the screen through the compositor.

use crate::LOG;
use wana_log::info;
use wana_text::bidi::Base;
use wana_text::font::Font;
pub use wana_text::fonts::DEFAULT_DIR;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::raster::{draw, Canvas};
use wana_text::{fonts, sha256};

/// Two lines at this size and width (right-aligned: an Arabic paragraph).
pub const TEXT: &str = "مرحبا بك في وانا، نظام تشغيل مستقل Wana OS 2026";
const SIZE: f32 = 40.0;
pub const WIDTH: u32 = 600;
const MARGIN: f32 = 20.0;
/// Panel and ink colors (the panel differs from the compositor background).
pub const PANEL: u32 = 0x243B6B;
pub const INK: u32 = 0xFFFFFF;

/// Renders the text; returns the canvas and one pixel fully covered by ink
/// (a 3x3 block of pure ink, for the screenshot check).
pub fn render(dir: &std::path::Path) -> Result<(Canvas, (u32, u32)), String> {
    fonts::verify_dir(dir)?;
    let set = FontSet {
        fonts: vec![
            Font::load(&dir.join("NotoSans-VF.ttf"))?,
            Font::load(&dir.join("NotoSansArabic-VF.ttf"))?,
        ],
    };
    let style = Style {
        size: SIZE,
        base: Base::Auto,
        align: Align::Start,
        width: Some(WIDTH as f32 - 2.0 * MARGIN),
        language: "ar".into(),
    };
    let l = layout(TEXT, &set, &style)?;
    let height = (l.height + 2.0 * MARGIN).ceil() as u32;
    let mut c = Canvas::new(WIDTH, height, PANEL);
    draw(&mut c, &l, &set, SIZE, MARGIN, MARGIN, INK);
    let ink = c.pixels.iter().filter(|&&p| p & 0xFF_FFFF != PANEL).count();
    let solid = find_solid(&c).ok_or("no fully inked pixel")?;
    info!(
        LOG,
        "client: text rendered: {} lines, {}x{}, {} ink pixels, sha256 {}",
        l.lines.len(),
        c.width,
        c.height,
        ink,
        sha256::hex(&sha256::digest(&c.bytes()))
    );
    Ok((c, solid))
}

/// The first pixel (row-major) whose 3x3 neighbourhood is all ink.
fn find_solid(c: &Canvas) -> Option<(u32, u32)> {
    let at = |x: u32, y: u32| c.pixels[(y * c.width + x) as usize] & 0xFF_FFFF;
    for y in 1..c.height - 1 {
        for x in 1..c.width - 1 {
            if (0..3).all(|dy| (0..3).all(|dx| at(x + dx - 1, y + dy - 1) == INK)) {
                return Some((x, y));
            }
        }
    }
    None
}
