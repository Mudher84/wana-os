//! What the shell draws, as pure functions from state to pixels, so the
//! design is unit-tested and a rendering can be checked by its hash.
//!
//! - Desktop: a vertical gradient (integer interpolation, exact).
//! - Top bar (RTL, as the UI is Arabic): "وانا" at the start (right), the
//!   time at the end (left) in Arabic-Indic digits.
//! - Launcher: a panel with a header and one row per app, the selected row
//!   in the accent color.

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
/// Width of the bar's start (right end) that opens the launcher on a click.
pub const BRAND_HIT: u32 = 160;

pub const LAUNCHER_BG: u32 = 0x1E2638;
pub const LAUNCHER_BORDER: u32 = 0x3A4560;
pub const LAUNCHER_DIM: u32 = 0x9AA4B8;
pub const ACCENT: u32 = 0x4F8CFF;
pub const LAUNCHER_WIDTH: u32 = 480;
pub const LAUNCHER_HEADER: u32 = 56;
pub const LAUNCHER_ROW: u32 = 48;
const LAUNCHER_BOTTOM: u32 = 8;
const LAUNCHER_TEXT: f32 = 20.0;
const LAUNCHER_PADDING: f32 = 24.0;
/// The launcher's title.
pub const APPS_TITLE: &str = "التطبيقات";

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

/// Fills a rectangle (clipped to the canvas).
fn fill(c: &mut Canvas, x: u32, y: u32, w: u32, h: u32, rgb: u32) {
    for row in y..(y + h).min(c.height) {
        let start = (row * c.width + x.min(c.width)) as usize;
        let end = (row * c.width + (x + w).min(c.width)) as usize;
        c.pixels[start..end].fill(0xFF00_0000 | rgb);
    }
}

/// Height of a launcher listing `rows` apps.
pub fn launcher_height(rows: usize) -> u32 {
    LAUNCHER_HEADER + LAUNCHER_ROW * rows as u32 + LAUNCHER_BOTTOM
}

/// The row at surface-local height `y`, if any.
pub fn launcher_row_at(y: f64, rows: usize) -> Option<usize> {
    let r = (y - f64::from(LAUNCHER_HEADER)) / f64::from(LAUNCHER_ROW);
    (r >= 0.0 && (r as usize) < rows).then_some(r as usize)
}

/// Draws RTL `text` right-aligned in the band from `top`, `height` tall.
fn line(
    c: &mut Canvas,
    fonts: &FontSet,
    text: &str,
    top: u32,
    height: u32,
    rgb: u32,
) -> Result<(), String> {
    let style = Style {
        size: LAUNCHER_TEXT,
        base: Base::Rtl,
        align: Align::Start,
        width: Some(c.width as f32 - 2.0 * LAUNCHER_PADDING),
        language: "ar".into(),
    };
    let l = layout(text, fonts, &style)?;
    let y = top as f32 + (height as f32 - l.height) / 2.0;
    draw(c, &l, fonts, LAUNCHER_TEXT, LAUNCHER_PADDING, y, rgb);
    Ok(())
}

/// The launcher: a title, then `names` one per row, `selected` highlighted.
pub fn launcher(fonts: &FontSet, names: &[&str], selected: usize) -> Result<Canvas, String> {
    let (w, h) = (LAUNCHER_WIDTH, launcher_height(names.len()));
    let mut c = Canvas::new(w, h, LAUNCHER_BORDER);
    fill(&mut c, 1, 1, w - 2, h - 2, LAUNCHER_BG);
    line(&mut c, fonts, APPS_TITLE, 0, LAUNCHER_HEADER, LAUNCHER_DIM)?;
    for (i, name) in names.iter().enumerate() {
        let top = LAUNCHER_HEADER + LAUNCHER_ROW * i as u32;
        if i == selected {
            fill(&mut c, 8, top + 2, w - 16, LAUNCHER_ROW - 4, ACCENT);
        }
        line(&mut c, fonts, name, top, LAUNCHER_ROW, BAR_TEXT)?;
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

    #[test]
    fn launcher_highlights_the_selected_row() {
        let names = ["نافذة تجريبية", "نص عربي"];
        let c = launcher(&fonts(), &names, 1).unwrap();
        assert_eq!((c.width, c.height), (LAUNCHER_WIDTH, launcher_height(2)));
        let at = |x: u32, y: u32| c.pixels[(y * c.width + x) as usize] & 0xFF_FFFF;
        assert_eq!(at(0, 0), LAUNCHER_BORDER);
        // Left side of the rows (text is at the right, RTL).
        let row = |i: u32| LAUNCHER_HEADER + LAUNCHER_ROW * i + LAUNCHER_ROW / 2;
        assert_eq!(at(20, row(0)), LAUNCHER_BG);
        assert_eq!(at(20, row(1)), ACCENT);
        let ink = |y0: u32, y1: u32, x0: u32, x1: u32| {
            (y0..y1).any(|y| (x0..x1).any(|x| ![LAUNCHER_BG, ACCENT].contains(&at(x, y))))
        };
        assert!(ink(0, LAUNCHER_HEADER, 300, 456), "title at the right");
        assert!(
            ink(row(0) - 10, row(0) + 10, 300, 456),
            "first name at the right"
        );
        assert!(
            !ink(row(0) - 10, row(0) + 10, 24, 200),
            "left of a short name stays empty"
        );
        assert_ne!(c, launcher(&fonts(), &names, 0).unwrap());
        assert_eq!(launcher_row_at(10.0, 2), None);
        assert_eq!(launcher_row_at(f64::from(row(1)), 2), Some(1));
        assert_eq!(
            launcher_row_at(f64::from(LAUNCHER_HEADER + 2 * LAUNCHER_ROW), 2),
            None
        );
    }
}
