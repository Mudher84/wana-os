//! wana-text: text-stack bring-up tool and boot test (decision 0002).
//!
//! Step 1: verifies the font directory against its SHA256SUMS, loads every
//! listed font through HarfBuzz and logs what it is (family, glyphs, units
//! per em, variation axes) and which of Wana's scripts it covers. Exits 0
//! only if every font verifies and loads, and Arabic and Latin are both
//! covered by some font.
//!
//! Usage: wana-text [--fonts DIR]   (default /usr/share/fonts/wana)

use std::path::PathBuf;
use std::process::ExitCode;
use wana_log::{error, info, Subsystem};
use wana_text::font::{Font, ARABIC_SAMPLE, LATIN_SAMPLE};
use wana_text::fonts;

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
    Ok(())
}
