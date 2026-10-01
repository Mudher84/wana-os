//! Deterministic Wana control-center rendering.

use crate::store::Notification;
use wana_text::bidi::Base;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::raster::{draw, Canvas};

pub const WIDTH: u32 = 820;
pub const HEIGHT: u32 = 600;

const BG: u32 = wana_theme::color::BG_DARK;
const CARD: u32 = wana_theme::color::CARD_DARK;
const TEXT: u32 = wana_theme::color::TEXT_DARK;
const DIM: u32 = wana_theme::color::DIM_DARK;
const ACCENT: u32 = wana_theme::color::ACCENT_TEAL;
const ALERT: u32 = wana_theme::color::INFO_BLUE;

fn fill(canvas: &mut Canvas, x: u32, y: u32, width: u32, height: u32, rgb: u32) {
    for row in y..(y + height).min(canvas.height) {
        let start = (row * canvas.width + x.min(canvas.width)) as usize;
        let end = (row * canvas.width + (x + width).min(canvas.width)) as usize;
        canvas.pixels[start..end].fill(0xFF00_0000 | rgb);
    }
}

fn label(
    canvas: &mut Canvas,
    fonts: &FontSet,
    value: &str,
    position: (f32, f32),
    width: f32,
    size: f32,
    rgb: u32,
) -> Result<(), String> {
    let (x, y) = position;
    let style = Style {
        size,
        base: Base::Rtl,
        align: Align::Start,
        width: Some(width),
        language: "ar".into(),
    };
    let shaped = layout(value, fonts, &style)?;
    draw(canvas, &shaped, fonts, size, x, y, rgb);
    Ok(())
}

fn short_app(app: &str) -> &str {
    app.rsplit('.').next().unwrap_or(app)
}

pub fn center(fonts: &FontSet, notifications: &[Notification]) -> Result<Canvas, String> {
    let mut canvas = Canvas::new(WIDTH, HEIGHT, BG);
    label(
        &mut canvas,
        fonts,
        "مركز التحكم",
        (36.0, 24.0),
        WIDTH as f32 - 72.0,
        30.0,
        TEXT,
    )?;
    label(
        &mut canvas,
        fonts,
        "الإشعارات وحالة النظام في مكان واحد",
        (36.0, 68.0),
        WIDTH as f32 - 72.0,
        16.0,
        DIM,
    )?;

    let card_width = 236;
    for (index, (title, value)) in [
        ("الشبكة", "جاهزة"),
        ("الأذونات", "محمية"),
        ("الإعدادات", "محلية"),
    ]
    .iter()
    .enumerate()
    {
        let x = 28 + index as u32 * 254;
        fill(&mut canvas, x, 108, card_width, 104, CARD);
        label(
            &mut canvas,
            fonts,
            title,
            ((x + 18) as f32, 122.0),
            (card_width - 36) as f32,
            18.0,
            TEXT,
        )?;
        label(
            &mut canvas,
            fonts,
            value,
            ((x + 18) as f32, 160.0),
            (card_width - 36) as f32,
            16.0,
            ACCENT,
        )?;
    }

    fill(&mut canvas, 28, 232, WIDTH - 56, 334, CARD);
    let heading = format!("الإشعارات — {}", notifications.len());
    label(
        &mut canvas,
        fonts,
        &heading,
        (48.0, 246.0),
        WIDTH as f32 - 96.0,
        22.0,
        TEXT,
    )?;

    if notifications.is_empty() {
        label(
            &mut canvas,
            fonts,
            "لا توجد إشعارات",
            (48.0, 300.0),
            WIDTH as f32 - 96.0,
            17.0,
            DIM,
        )?;
    } else {
        for (index, notification) in notifications.iter().rev().take(4).enumerate() {
            let top = 292.0 + index as f32 * 64.0;
            let title = format!(
                "#{}  {} — {}",
                notification.seq,
                short_app(&notification.app),
                notification.title
            );
            label(
                &mut canvas,
                fonts,
                &title,
                (48.0, top),
                WIDTH as f32 - 96.0,
                16.0,
                ALERT,
            )?;
            label(
                &mut canvas,
                fonts,
                &notification.body,
                (48.0, top + 27.0),
                WIDTH as f32 - 96.0,
                14.0,
                DIM,
            )?;
        }
    }

    Ok(canvas)
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
                .map(|name| Font::load(&dir.join(name)).unwrap())
                .collect(),
        }
    }

    #[test]
    fn center_is_deterministic_and_notifications_change_pixels() {
        let list = vec![Notification {
            seq: 1,
            app: "org.wana.Network".into(),
            title: "الشبكة متصلة".into(),
            body: "تم الاتصال بالشبكة المحلية".into(),
        }];
        let first = center(&fonts(), &list).unwrap();
        let second = center(&fonts(), &list).unwrap();
        assert_eq!(first, second);
        assert_eq!((first.width, first.height), (WIDTH, HEIGHT));
        assert_ne!(first, center(&fonts(), &[]).unwrap());
    }
}
