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
//! Usage: wana-text [--fonts DIR]   (default /usr/share/fonts/wana)

use std::path::PathBuf;
use std::process::ExitCode;
use wana_log::{error, info, Subsystem};
use wana_text::bidi::{self, Base};
use wana_text::font::{Font, ARABIC_SAMPLE, LATIN_SAMPLE};
use wana_text::fonts;
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
    bidi_check()
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
