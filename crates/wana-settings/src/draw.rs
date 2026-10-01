//! Deterministic Settings UI rendering.

use crate::config::Settings;
use wana_text::bidi::Base;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::raster::{draw, Canvas};

pub const WIDTH: u32 = 720;
pub const HEIGHT: u32 = 480;
const BG_DARK: u32 = wana_theme::color::BG_DARK;
const BG_LIGHT: u32 = wana_theme::color::BG_LIGHT;
const CARD_DARK: u32 = wana_theme::color::CARD_DARK;
const CARD_LIGHT: u32 = wana_theme::color::CARD_LIGHT;
const TEXT_DARK: u32 = wana_theme::color::TEXT_DARK;
const TEXT_LIGHT: u32 = wana_theme::color::TEXT_LIGHT;
const DIM_DARK: u32 = wana_theme::color::DIM_DARK;
const DIM_LIGHT: u32 = wana_theme::color::DIM_LIGHT;

fn fill(c: &mut Canvas, x: u32, y: u32, w: u32, h: u32, rgb: u32) {
    for row in y..(y + h).min(c.height) {
        let start = (row * c.width + x.min(c.width)) as usize;
        let end = (row * c.width + (x + w).min(c.width)) as usize;
        c.pixels[start..end].fill(0xFF00_0000 | rgb);
    }
}

fn text(
    c: &mut Canvas,
    fonts: &FontSet,
    value: &str,
    y: f32,
    size: f32,
    rgb: u32,
) -> Result<(), String> {
    let style = Style {
        size,
        base: Base::Rtl,
        align: Align::Start,
        width: Some(c.width as f32 - 80.0),
        language: "ar".into(),
    };
    let l = layout(value, fonts, &style)?;
    draw(c, &l, fonts, size, 40.0, y, rgb);
    Ok(())
}

pub fn settings(fonts: &FontSet, s: &Settings) -> Result<Canvas, String> {
    let light = s.theme == "light";
    let bg = if light { BG_LIGHT } else { BG_DARK };
    let card = if light { CARD_LIGHT } else { CARD_DARK };
    let fg = if light { TEXT_LIGHT } else { TEXT_DARK };
    let dim = if light { DIM_LIGHT } else { DIM_DARK };

    let mut c = Canvas::new(WIDTH, HEIGHT, bg);
    text(&mut c, fonts, "الإعدادات", 28.0, 30.0, fg)?;
    text(
        &mut c,
        fonts,
        "إعدادات وانا الأساسية محفوظة محلياً وبشكل ذري",
        72.0,
        17.0,
        dim,
    )?;

    for (i, (name, value)) in [
        (
            "اللغة",
            if s.language == "ar" {
                "العربية"
            } else {
                "English"
            },
        ),
        (
            "المظهر",
            if s.theme == "dark" {
                "داكن"
            } else {
                "فاتح"
            },
        ),
        (
            "لون التمييز",
            match s.accent.as_str() {
                "teal" => "فيروزي",
                "violet" => "بنفسجي",
                _ => "أزرق",
            },
        ),
        ("المنطقة الزمنية", s.timezone.as_str()),
    ]
    .iter()
    .enumerate()
    {
        let y = 112 + i as u32 * 82;
        fill(&mut c, 32, y, WIDTH - 64, 70, card);
        text(&mut c, fonts, name, y as f32 + 10.0, 20.0, fg)?;
        text(&mut c, fonts, value, y as f32 + 38.0, 16.0, dim)?;
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wana_text::font::Font;

    fn fonts() -> FontSet {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/fonts");
        wana_text::fonts::verify_dir(&dir).expect("pinned fonts");
        FontSet {
            fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
                .iter()
                .map(|n| Font::load(&dir.join(n)).unwrap())
                .collect(),
        }
    }

    #[test]
    fn rendering_is_deterministic_and_theme_changes_pixels() {
        let dark = settings(&fonts(), &Settings::default()).unwrap();
        assert_eq!((dark.width, dark.height), (WIDTH, HEIGHT));
        assert_eq!(dark, settings(&fonts(), &Settings::default()).unwrap());
        let light = Settings {
            theme: "light".into(),
            ..Settings::default()
        };
        assert_ne!(dark, settings(&fonts(), &light).unwrap());
    }
}
