//! wana-text: text-stack bring-up tool and boot test (decision 0002).
//!
//! Step 1: verifies the font directory against its SHA256SUMS, loads every
//! listed font through HarfBuzz and logs what it is (family, glyphs, units
//! per em, variation axes) and which of Wana's scripts it covers. Exits 0
//! only if every font verifies and loads, and Arabic and Latin are both
//! covered by some font.
//!
//! Step 2: shapes Arabic samples with Noto Naskh Arabic and logs the glyphs
//! (joined forms, lam-alef ligature forms) and resolves the BiDi levels
//! and visual run order of a mixed Arabic/Latin/number line. Exits 1 if
//! Arabic does not join or the BiDi order is wrong.
//!
//! Step 3: lays out an Arabic line with Latin and digits in a narrow width
//! (fonts from the fallback chain, line breaks, right alignment) and checks
//! that the carets move right to left and clicks map back.
//!
//! Usage: wana-text [--fonts DIR]   (default /usr/share/fonts/wana)

use std::path::PathBuf;
use std::process::ExitCode;
use wana_log::{error, info, Subsystem};
use wana_text::bidi::{self, Base};
use wana_text::font::{Font, ARABIC_SAMPLE, LATIN_SAMPLE};
use wana_text::fonts;
use wana_text::layout::{layout, Align, FontSet, Style};
use wana_text::shape::{shape, width, Glyph};

const RENDER: Subsystem = Subsystem::Render;

fn main() -> ExitCode {
    wana_log::init_from_env();
    let mut dir = PathBuf::from(fonts::DEFAULT_DIR);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--fonts" => match it.next() {
                Some(d) => dir = d.into(),
                None => {
                    error!(RENDER, "--fonts needs a directory");
                    return ExitCode::from(2);
                }
            },
            other => {
                error!(RENDER, "unknown argument {other}");
                return ExitCode::from(2);
            }
        }
    }
    match run(&dir) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            error!(RENDER, "{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(dir: &std::path::Path) -> Result<(), String> {
    info!(
        RENDER,
        "wana-text {}: fonts in {}",
        env!("CARGO_PKG_VERSION"),
        dir.display()
    );
    let verified = fonts::verify_dir(dir)?;
    let (mut arabic, mut latin) = (false, false);
    for v in &verified {
        let name = v.path.file_name().unwrap_or_default().to_string_lossy();
        info!(
            RENDER,
            "font verified: {name} ({} bytes, sha256 {})", v.bytes, v.sha256
        );
        let f = Font::load(&v.path)?;
        let axes: Vec<String> = f
            .axes()
            .iter()
            .map(|a| format!("{} {}..{} (default {})", a.tag, a.min, a.max, a.default))
            .collect();
        let covers_ar = f.covers(ARABIC_SAMPLE);
        let covers_lat = f.covers(LATIN_SAMPLE);
        arabic |= covers_ar;
        latin |= covers_lat;
        let scripts: Vec<&str> = [("Arabic", covers_ar), ("Latin", covers_lat)]
            .iter()
            .filter(|(_, c)| *c)
            .map(|(s, _)| *s)
            .collect();
        info!(
            RENDER,
            "font loaded: {name}: \"{}\", {} glyphs, {} units/em, axes [{}], covers [{}]",
            f.family(),
            f.glyph_count(),
            f.units_per_em(),
            axes.join(", "),
            scripts.join(", ")
        );
    }
    if !(arabic && latin) {
        return Err(format!(
            "fonts do not cover Wana's scripts (Arabic: {arabic}, Latin: {latin})"
        ));
    }
    info!(
        RENDER,
        "fonts ok: {} verified and loaded, Arabic and Latin covered",
        verified.len()
    );
    shaping(&dir.join("NotoNaskhArabic-VF.ttf"))?;
    bidi_check()?;
    layout_check(dir)
}

fn layout_check(dir: &std::path::Path) -> Result<(), String> {
    let set = FontSet {
        fonts: vec![
            Font::load(&dir.join("NotoSans-VF.ttf"))?,
            Font::load(&dir.join("NotoSansArabic-VF.ttf"))?,
        ],
    };
    let text = "مرحبا بك في Wana 2026";
    let width = 120.0;
    let style = Style {
        size: 20.0,
        base: Base::Auto,
        align: Align::Start,
        width: Some(width),
        language: "ar".into(),
    };
    let l = layout(text, &set, &style)?;
    let lines: Vec<String> = l
        .lines
        .iter()
        .map(|ln| {
            let fonts: std::collections::BTreeSet<usize> =
                ln.glyphs.iter().map(|g| g.font).collect();
            format!(
                "\"{}\" {:.1}px fonts {:?}",
                &text[ln.start..ln.content_end],
                ln.width,
                fonts
            )
        })
        .collect();
    let fits = l.lines.iter().all(|ln| ln.width <= width + 0.001);
    let right = l
        .lines
        .iter()
        .all(|ln| ln.rtl && (ln.x + ln.width - width).abs() < 0.001);
    // Carets on the first line move right to left; every caret's click
    // position maps back to a caret at the same place.
    let first = &l.lines[0];
    let offs: Vec<usize> = text[first.start..first.content_end]
        .char_indices()
        .map(|(i, _)| first.start + i)
        .chain([first.content_end])
        .collect();
    let xs: Vec<f32> = offs.iter().map(|&o| l.caret(o).1).collect();
    let rtl_carets = xs.windows(2).all(|w| w[0] > w[1]);
    let round_trip = (0..=text.len())
        .filter(|&o| text.is_char_boundary(o))
        .all(|o| {
            let (li, x) = l.caret(o);
            (l.caret(l.hit(li, x)).1 - x).abs() < 0.001
        });
    info!(
        RENDER,
        "layout \"{text}\" at 20px in {width}px: {} lines: {}",
        l.lines.len(),
        lines.join(" / ")
    );
    info!(
        RENDER,
        "layout checks: fits {fits}, right-aligned {right}, carets right to left {rtl_carets}, clicks round-trip {round_trip}"
    );
    if !(fits && right && rtl_carets && round_trip) {
        return Err("layout checks failed".into());
    }
    info!(
        RENDER,
        "text ok: layout (line breaks, fallback fonts, alignment, carets)"
    );
    Ok(())
}

fn ids(g: &[Glyph]) -> String {
    let v: Vec<String> = g.iter().map(|g| g.id.to_string()).collect();
    v.join(" ")
}

/// The glyph carrying the advance of `cluster` (the letter body; dots are
/// separate zero-advance glyphs in the Noto Arabic fonts).
fn body(g: &[Glyph], cluster: u32) -> Option<u32> {
    g.iter()
        .find(|g| g.cluster == cluster && g.x_advance > 0)
        .map(|g| g.id)
}

fn shaping(naskh: &std::path::Path) -> Result<(), String> {
    let f = Font::load(naskh)?;
    let text = "مرحبا";
    let g = shape(&f, text, true, "ar")?;
    let joined = g.iter().all(|gl| {
        let c = text[gl.cluster as usize..].chars().next();
        c.and_then(|c| f.glyph(c)) != Some(gl.id)
    });
    info!(
        RENDER,
        "shaped \"{text}\" ({}, RTL): {} glyphs [{}], width {} units, joined forms: {}",
        f.family(),
        g.len(),
        ids(&g),
        width(&g),
        if joined { "yes" } else { "NO" }
    );
    if !joined {
        return Err(format!("\"{text}\" was not joined: glyphs [{}]", ids(&g)));
    }
    let la = shape(&f, "لا", true, "ar")?;
    let lam_initial = body(&shape(&f, "لم", true, "ar")?, 0);
    let alef_final = body(&shape(&f, "با", true, "ar")?, 2);
    let (lam, alef) = (body(&la, 0), body(&la, 2));
    let ligature = lam.is_some() && lam != lam_initial && alef.is_some() && alef != alef_final;
    info!(
        RENDER,
        "shaped \"لا\": lam {} (initial lam {}), alef {} (final alef {}): lam-alef forms: {}",
        lam.unwrap_or(0),
        lam_initial.unwrap_or(0),
        alef.unwrap_or(0),
        alef_final.unwrap_or(0),
        if ligature { "yes" } else { "NO" }
    );
    if !ligature {
        return Err("lam-alef was not formed".into());
    }
    Ok(())
}

fn bidi_check() -> Result<(), String> {
    let text = "Wana 2026 وانا";
    let p = bidi::paragraph(text, Base::Rtl)?;
    let runs = bidi::runs(&p.levels);
    let order = bidi::visual_order(&runs);
    let levels: String = p.levels.iter().map(|l| char::from(b'0' + l)).collect();
    let chars: Vec<char> = text.chars().collect();
    let shown: Vec<String> = order
        .iter()
        .map(|&i| {
            let r = runs[i];
            let s: String = chars[r.start..r.end].iter().collect();
            format!("\"{}\" {}", s.trim(), if r.rtl() { "RTL" } else { "LTR" })
        })
        .collect();
    info!(
        RENDER,
        "bidi \"{text}\" (RTL paragraph): levels {levels}, left to right: {}",
        shown.join(" | ")
    );
    if levels != "22222222211111" || order != [1, 0] {
        return Err(format!(
            "unexpected BiDi result: levels {levels}, order {order:?}"
        ));
    }
    info!(
        RENDER,
        "text ok: fonts, Arabic shaping and BiDi (FriBidi, UAX #9)"
    );
    Ok(())
}
