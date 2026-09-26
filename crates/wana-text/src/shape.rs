//! Shaping: one run of text in one direction and one font becomes
//! positioned glyphs (HarfBuzz). For Arabic this is where letters take their
//! joined forms (initial, medial, final), lam-alef becomes one ligature, and
//! marks are positioned.
//!
//! Units: font units (the font's units per em) until layout scales them.

use crate::ffi;
use crate::font::Font;
use std::ptr::NonNull;

/// One positioned glyph. `cluster` is the byte offset in the shaped text of
/// the first character the glyph came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub id: u32,
    pub cluster: u32,
    pub x_advance: i32,
    pub y_advance: i32,
    pub x_offset: i32,
    pub y_offset: i32,
}

/// Shapes `text` as one run. `rtl` comes from the BiDi level of the run;
/// the script is detected from the text. Glyphs are returned in visual
/// order (left to right), as HarfBuzz outputs them, so an RTL run's
/// clusters decrease.
pub fn shape(font: &Font, text: &str, rtl: bool, language: &str) -> Result<Vec<Glyph>, String> {
    let len = std::os::raw::c_int::try_from(text.len()).map_err(|_| "text too long".to_string())?;
    // SAFETY: plain constructor; the result is checked below.
    let buf = NonNull::new(unsafe { ffi::hb_buffer_create() }).ok_or("hb_buffer_create failed")?;
    let lang = std::ffi::CString::new(language).map_err(|_| "NUL in language".to_string())?;
    // SAFETY: buf is valid until destroyed at the end; text and lang
    // outlive the calls that read them (HarfBuzz copies the text); the
    // info and position arrays are owned by buf and read before it is
    // destroyed, with the length HarfBuzz reports.
    let glyphs = unsafe {
        let b = buf.as_ptr();
        ffi::hb_buffer_set_cluster_level(b, ffi::HB_BUFFER_CLUSTER_LEVEL_MONOTONE_CHARACTERS);
        ffi::hb_buffer_add_utf8(b, text.as_ptr().cast(), len, 0, len);
        ffi::hb_buffer_set_direction(
            b,
            if rtl {
                ffi::HB_DIRECTION_RTL
            } else {
                ffi::HB_DIRECTION_LTR
            },
        );
        ffi::hb_buffer_set_language(b, ffi::hb_language_from_string(lang.as_ptr(), -1));
        ffi::hb_buffer_guess_segment_properties(b);
        ffi::hb_shape(font.raw(), b, std::ptr::null(), 0);
        let mut n = 0;
        let infos = ffi::hb_buffer_get_glyph_infos(b, &mut n);
        let mut m = 0;
        let pos = ffi::hb_buffer_get_glyph_positions(b, &mut m);
        let out = if n == 0 || infos.is_null() || pos.is_null() || m != n {
            Vec::new()
        } else {
            let infos = std::slice::from_raw_parts(infos, n as usize);
            let pos = std::slice::from_raw_parts(pos, n as usize);
            infos
                .iter()
                .zip(pos)
                .map(|(i, p)| Glyph {
                    id: i.codepoint,
                    cluster: i.cluster,
                    x_advance: p.x_advance,
                    y_advance: p.y_advance,
                    x_offset: p.x_offset,
                    y_offset: p.y_offset,
                })
                .collect()
        };
        ffi::hb_buffer_destroy(b);
        out
    };
    Ok(glyphs)
}

/// Total advance of a shaped run (font units).
pub fn width(glyphs: &[Glyph]) -> i32 {
    glyphs.iter().map(|g| g.x_advance).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::test_fonts_dir;

    fn font(name: &str) -> Font {
        let dir = test_fonts_dir();
        crate::fonts::verify_dir(&dir).expect("pinned fonts verify");
        Font::load(&dir.join(name)).unwrap()
    }

    /// The character each glyph came from.
    fn source(text: &str, g: &Glyph) -> char {
        text[g.cluster as usize..].chars().next().unwrap()
    }

    /// The glyph that carries a cluster's advance (its base). The Noto Arabic
    /// fonts draw dotted letters as a dotless body plus separate dot glyphs
    /// with zero advance, positioned like marks.
    fn base(glyphs: &[Glyph], cluster: u32) -> Glyph {
        *glyphs
            .iter()
            .find(|g| g.cluster == cluster && g.x_advance > 0)
            .unwrap_or_else(|| panic!("no base glyph for cluster {cluster}"))
    }

    #[test]
    fn arabic_letters_take_their_joined_forms() {
        let naskh = font("NotoNaskhArabic-VF.ttf");
        let text = "مرحبا";
        let g = shape(&naskh, text, true, "ar").unwrap();
        // Visual order for RTL: the last letter (alef) comes first; each
        // cluster's glyphs are together.
        let mut clusters: Vec<u32> = g.iter().map(|g| g.cluster).collect();
        clusters.dedup();
        assert_eq!(clusters, [8, 6, 4, 2, 0]);
        // Every letter here joins a neighbour (meem initial, reh final, hah
        // initial, beh medial, alef final): no glyph may be the isolated
        // form the character map gives.
        for gl in &g {
            let c = source(text, gl);
            assert_ne!(Some(gl.id), naskh.glyph(c), "{c} must not be isolated");
        }
        // Beh is a dotless body plus its dot (zero advance, offset).
        let beh: Vec<&Glyph> = g.iter().filter(|g| g.cluster == 6).collect();
        assert_eq!(beh.len(), 2);
        assert!(beh.iter().any(|g| g.x_advance == 0 && g.x_offset != 0));
    }

    #[test]
    fn isolated_letters_keep_their_nominal_glyphs() {
        let naskh = font("NotoNaskhArabic-VF.ttf");
        let text = "م ر";
        let g = shape(&naskh, text, true, "ar").unwrap();
        for gl in g.iter().filter(|g| source(text, g) != ' ') {
            let c = source(text, gl);
            assert_eq!(Some(gl.id), naskh.glyph(c), "{c} stands alone");
        }
    }

    #[test]
    fn lam_alef_uses_its_ligature_forms() {
        // Lam followed by alef must not be drawn as an ordinary initial lam
        // joined to an ordinary final alef. These fonts build the ligature
        // from two dedicated glyphs.
        for name in ["NotoNaskhArabic-VF.ttf", "NotoSansArabic-VF.ttf"] {
            let f = font(name);
            let la = shape(&f, "لا", true, "ar").unwrap();
            let (lam, alef) = (base(&la, 0), base(&la, 2));
            let lam_initial = base(&shape(&f, "لم", true, "ar").unwrap(), 0);
            let alef_final = base(&shape(&f, "با", true, "ar").unwrap(), 2);
            assert_ne!(lam.id, lam_initial.id, "{name}: lam before alef");
            assert_ne!(alef.id, alef_final.id, "{name}: alef after lam");
            assert_ne!(Some(lam.id), f.glyph('ل'));
            assert_ne!(Some(alef.id), f.glyph('ا'));
        }
    }

    #[test]
    fn same_letter_differs_by_position() {
        // beh isolated ("ب"), and in "بب" the first is initial and the
        // second final: three different bodies.
        for name in ["NotoNaskhArabic-VF.ttf", "NotoSansArabic-VF.ttf"] {
            let f = font(name);
            let iso = base(&shape(&f, "ب", true, "ar").unwrap(), 0);
            let pair = shape(&f, "بب", true, "ar").unwrap();
            let (ini, fin) = (base(&pair, 0), base(&pair, 2));
            let ids = [iso.id, ini.id, fin.id];
            assert!(
                ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2],
                "{name}: three forms {ids:?}"
            );
        }
    }

    #[test]
    fn latin_runs_left_to_right() {
        let sans = font("NotoSans-VF.ttf");
        let text = "Wana";
        let g = shape(&sans, text, false, "en").unwrap();
        let clusters: Vec<u32> = g.iter().map(|g| g.cluster).collect();
        assert_eq!(clusters, [0, 1, 2, 3]);
        for gl in &g {
            assert_eq!(Some(gl.id), sans.glyph(source(text, gl)));
            assert!(gl.x_advance > 0);
        }
        assert!(width(&g) > 1000, "four letters are wider than one em");
    }

    #[test]
    fn empty_text_has_no_glyphs() {
        let sans = font("NotoSans-VF.ttf");
        assert!(shape(&sans, "", false, "en").unwrap().is_empty());
    }
}
