use crate::settings::Settings;
use wana_text::bidi::Base;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::raster::{draw, Canvas};

const BG: u32 = 0x111827;
const PANEL: u32 = 0x1B2433;
const TEXT: u32 = 0xF3F4F6;
const DIM: u32 = 0x9CA3AF;
const ACCENT: u32 = 0x4F8CFF;

fn fill(c: &mut Canvas, x: u32, y: u32, w: u32, h: u32, rgb: u32) {
    for yy in y..(y + h).min(c.height) {
        let a = (yy * c.width + x.min(c.width)) as usize;
        let b = (yy * c.width + (x + w).min(c.width)) as usize;
        c.pixels[a..b].fill(0xFF00_0000 | rgb);
    }
}

fn text(
    c: &mut Canvas,
    fonts: &FontSet,
    value: &str,
    x: f32,
    y: f32,
    width: f32,
    size: f32,
    color: u32,
) -> Result<(), String> {
    let style = Style {
        size,
        base: Base::Rtl,
        align: Align::Start,
        width: Some(width),
        language: "ar".into(),
    };
    let l = layout(value, fonts, &style)?;
    draw(c, &l, fonts, size, x, y, color);
    Ok(())
}

pub fn screen(width: u32, height: u32, fonts: &FontSet, s: &Settings) -> Result<Canvas, String> {
    let mut c = Canvas::new(width, height, BG);
    let margin = 28;
    fill(&mut c, margin, 24, width.saturating_sub(margin * 2), 76, PANEL);
    text(
        &mut c,
        fonts,
        "الإعدادات",
        48.0,
        42.0,
        width.saturating_sub(96) as f32,
        30.0,
        TEXT,
    )?;

    let rows = [
        ("اللغة", if s.language == "ar" { "العربية" } else { "English" }.to_string()),
        ("المظهر", match s.theme.as_str() {
            "light" => "فاتح",
            "system" => "النظام",
            _ => "داكن",
        }.to_string()),
        ("اللون", format!("#{}", s.accent)),
        ("الوقت", format!("{} ساعة", s.clock)),
        ("لوحة المفاتيح", s.keyboard.clone()),
    ];
    let panel_y = 120;
    let row_h = 68;
    fill(
        &mut c,
        margin,
        panel_y,
        width.saturating_sub(margin * 2),
        row_h * rows.len() as u32 + 20,
        PANEL,
    );
    for (index, (label, value)) in rows.iter().enumerate() {
        let y = panel_y + 12 + index as u32 * row_h;
        if index > 0 {
            fill(&mut c, margin + 20, y, width.saturating_sub((margin + 20) * 2), 1, 0x334155);
        }
        text(
            &mut c,
            fonts,
            label,
            52.0,
            (y + 14) as f32,
            width.saturating_sub(104) as f32,
            21.0,
            TEXT,
        )?;
        text(
            &mut c,
            fonts,
            value,
            52.0,
            (y + 39) as f32,
            width.saturating_sub(104) as f32,
            15.0,
            if *label == "اللون" { ACCENT } else { DIM },
        )?;
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_are_stable_without_fonts() {
        let c = Canvas::new(720, 520, BG);
        assert_eq!((c.width, c.height), (720, 520));
    }
}
