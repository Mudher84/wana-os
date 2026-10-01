//! Deterministic native permission/audit center rendering.

use crate::store::{Audit, Decision, Rule};
use wana_text::bidi::Base;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::raster::{draw, Canvas};

pub const WIDTH: u32 = 820;
pub const HEIGHT: u32 = 600;


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
    y: f32,
    size: f32,
    rgb: u32,
) -> Result<(), String> {
    let style = Style {
        size,
        base: Base::Rtl,
        align: Align::Start,
        width: Some(canvas.width as f32 - 72.0),
        language: "ar".into(),
    };
    let shaped = layout(value, fonts, &style)?;
    draw(canvas, &shaped, fonts, size, 36.0, y, rgb);
    Ok(())
}

fn short_app(app: &str) -> &str {
    app.rsplit('.').next().unwrap_or(app)
}

fn permission_ar(permission: &str) -> &str {
    match permission {
        "files.read" => "قراءة الملفات",
        "files.write" => "تعديل الملفات",
        "network" => "الشبكة",
        "storage.raw" => "الكتابة على الأقراص",
        "notifications" => "الإشعارات",
        _ => permission,
    }
}

pub fn center(fonts: &FontSet, rules: &[Rule], audit: &[Audit]) -> Result<Canvas, String> {
    let palette = wana_theme::current();
    let mut canvas = Canvas::new(WIDTH, HEIGHT, palette.bg);
    label(
        &mut canvas,
        fonts,
        "مركز الأذونات والتدقيق",
        24.0,
        30.0,
        palette.text,
    )?;
    label(
        &mut canvas,
        fonts,
        "كل صلاحية غير مسجلة تُرفض تلقائياً — السجل محلي ومحدود",
        68.0,
        16.0,
        palette.dim,
    )?;

    fill(&mut canvas, 28, 112, WIDTH - 56, 184, palette.card);
    label(&mut canvas, fonts, "الأذونات", 124.0, 21.0, TEXT)?;
    if rules.is_empty() {
        label(&mut canvas, fonts, "لا توجد قواعد محفوظة", 166.0, 16.0, DIM)?;
    } else {
        for (index, rule) in rules.iter().take(4).enumerate() {
            let status = if rule.decision == Decision::Allow {
                "مسموح"
            } else {
                "مرفوض"
            };
            let color = if rule.decision == Decision::Allow {
                palette.accent
            } else {
                palette.danger
            };
            let line = format!(
                "{} — {} — {}",
                short_app(&rule.app),
                permission_ar(&rule.permission),
                status
            );
            label(
                &mut canvas,
                fonts,
                &line,
                164.0 + index as f32 * 30.0,
                15.0,
                color,
            )?;
        }
    }

    fill(&mut canvas, 28, 316, WIDTH - 56, 250, palette.card);
    label(&mut canvas, fonts, "آخر أحداث التدقيق", 328.0, 21.0, TEXT)?;
    if audit.is_empty() {
        label(&mut canvas, fonts, "السجل فارغ", 370.0, 16.0, DIM)?;
    } else {
        for (index, event) in audit.iter().rev().take(5).enumerate() {
            let status = if event.decision == Decision::Allow {
                "سماح"
            } else {
                "رفض"
            };
            let line = format!(
                "#{}  {}  {}  {}",
                event.seq,
                short_app(&event.app),
                permission_ar(&event.permission),
                status
            );
            label(
                &mut canvas,
                fonts,
                &line,
                368.0 + index as f32 * 34.0,
                15.0,
                if event.decision == Decision::Allow {
                    palette.accent
                } else {
                    palette.danger
                },
            )?;
        }
    }
    Ok(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Audit, Decision, Rule};
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
    fn center_is_deterministic_and_decisions_change_pixels() {
        let rules = vec![Rule {
            app: "org.wana.Files".into(),
            permission: "files.read".into(),
            decision: Decision::Allow,
        }];
        let audit = vec![Audit {
            seq: 1,
            action: "policy-set".into(),
            actor: "settings".into(),
            app: "org.wana.Files".into(),
            permission: "files.read".into(),
            decision: Decision::Allow,
        }];
        let first = center(&fonts(), &rules, &audit).unwrap();
        let second = center(&fonts(), &rules, &audit).unwrap();
        assert_eq!(first, second);
        assert_eq!((first.width, first.height), (WIDTH, HEIGHT));

        let denied = vec![Rule {
            decision: Decision::Deny,
            ..rules[0].clone()
        }];
        assert_ne!(first, center(&fonts(), &denied, &audit).unwrap());
    }
}
