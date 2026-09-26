//! What the shell draws, as pure functions from state to pixels, so the
//! design is unit-tested and a rendering can be checked by its hash.
//!
//! - Desktop: a vertical gradient (integer interpolation, exact).
//! - Top bar (RTL, as the UI is Arabic): "وانا" at the start (right), the
//!   time at the end (left) in Arabic-Indic digits.

use wana_text::bidi::Base;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::raster::{draw, Canvas};

pub const DESKTOP_TOP: u32 = 0x16213E;
pub const DESKTOP_BOTTOM: u32 = 0x1B3A5C;
pub const BAR: u32 = 0x0B0F1A;
pub const BAR_TEXT: u32 = 0xE8ECF4;
pub const BAR_HEIGHT: u32 = 40;
const BAR_TEXT_SIZE: f32 = 18.0;
const BAR_PADDING: f32 = 16.0;
/// The label at the bar's start.
pub const BRAND: &str = "وانا";

/// `a` to `b` at `i` of `n` (channels interpolated with rounding).
pub fn mix(a: u32, b: u32, i: u32, n: u32) -> u32 {
    if n == 0 {
        return a;
    }
    let ch = |shift: u32| {
        let (x, y) = ((a >> shift) & 0xFF, (b >> shift) & 0xFF);
        ((x * (n - i) + y * i + n / 2) / n) << shift
    };
    ch(16) | ch(8) | ch(0)
}

/// The desktop background: top color to bottom color, row by row.
pub fn desktop(width: u32, height: u32) -> Canvas {
    let mut c = Canvas::new(width, height, DESKTOP_TOP);
    let last = height.saturating_sub(1);
    for y in 0..height {
        let rgb = 0xFF00_0000 | mix(DESKTOP_TOP, DESKTOP_BOTTOM, y, last);
        let row = (y * width) as usize;
        c.pixels[row..row + width as usize].fill(rgb);
    }
    c
}

/// "16:20" with Arabic-Indic digits: "١٦:٢٠".
pub fn arabic_digits(s: &str) -> String {
    s.chars()
        .map(|c| match c.to_digit(10) {
            Some(d) => char::from_u32(0x0660 + d).unwrap_or(c),
            None => c,
        })
        .collect()
}

/// Hours and minutes (UTC) of a Unix time.
pub fn clock(unix_secs: u64) -> (u32, u32) {
    let day = unix_secs % 86_400;
    ((day / 3600) as u32, (day % 3600 / 60) as u32)
}

/// The top bar with `time` ("HH:MM", shown in Arabic-Indic digits).
pub fn bar(width: u32, fonts: &FontSet, time: &str) -> Result<Canvas, String> {
    let mut c = Canvas::new(width, BAR_HEIGHT, BAR);
    let text_width = width as f32 - 2.0 * BAR_PADDING;
    let style = |align| Style {
        size: BAR_TEXT_SIZE,
        base: Base::Rtl,
        align,
        width: Some(text_width),
        language: "ar".into(),
    };
    // Start (right in RTL): the brand; end (left): the time.
    for (text, align) in [
        (BRAND.to_string(), Align::Start),
        (arabic_digits(time), Align::End),
    ] {
        let l = layout(&text, fonts, &style(align))?;
        let top = (BAR_HEIGHT as f32 - l.height) / 2.0;
        draw(&mut c, &l, fonts, BAR_TEXT_SIZE, BAR_PADDING, top, BAR_TEXT);
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_runs_from_top_to_bottom_color() {
        let c = desktop(4, 5);
        assert_eq!(c.pixels[0] & 0xFF_FFFF, DESKTOP_TOP);
        assert_eq!(c.pixels[4 * 4] & 0xFF_FFFF, DESKTOP_BOTTOM);
        assert_eq!(mix(0x000000, 0x0000FF, 1, 2), 0x000080, "half way, rounded");
        assert_eq!(mix(0x102030, 0x405060, 0, 0), 0x102030);
        // Rows are uniform.
        assert!(c.pixels[4..8].iter().all(|p| *p == c.pixels[4]));
    }

    #[test]
    fn digits_become_arabic_indic() {
        assert_eq!(arabic_digits("16:20"), "١٦:٢٠");
        assert_eq!(arabic_digits("وانا 9"), "وانا ٩");
    }

    #[test]
    fn clock_is_utc_hours_and_minutes() {
        assert_eq!(clock(0), (0, 0));
        assert_eq!(clock(86_400 + 16 * 3600 + 20 * 60 + 59), (16, 20));
    }

    fn fonts() -> FontSet {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/fonts");
        wana_text::fonts::verify_dir(&dir).expect("pinned fonts (make fonts)");
        FontSet {
            fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
                .iter()
                .map(|n| wana_text::font::Font::load(&dir.join(n)).unwrap())
                .collect(),
        }
    }

    #[test]
    fn bar_puts_the_brand_right_and_the_time_left() {
        let c = bar(1280, &fonts(), "16:20").unwrap();
        let ink = |x0: u32, x1: u32| {
            (0..BAR_HEIGHT)
                .any(|y| (x0..x1).any(|x| c.pixels[(y * 1280 + x) as usize] & 0xFF_FFFF != BAR))
        };
        assert!(
            ink(1180, 1264),
            "brand at the right (the start of an RTL bar)"
        );
        assert!(ink(16, 100), "time at the left");
        assert!(!ink(400, 880), "nothing in the middle");
        assert!(!ink(0, 16) && !ink(1264, 1280), "padding kept");
        // Deterministic: the same inputs give the same pixels.
        assert_eq!(c, bar(1280, &fonts(), "16:20").unwrap());
        assert_ne!(c, bar(1280, &fonts(), "16:21").unwrap());
    }
}
