//! A font loaded through HarfBuzz: what it is and what it covers.

use crate::ffi;
use std::ffi::CString;
use std::path::Path;
use std::ptr::NonNull;

/// A variation axis (variable fonts), e.g. `wght` 100..900, default 400.
#[derive(Debug, Clone, PartialEq)]
pub struct Axis {
    pub tag: String,
    pub min: f32,
    pub default: f32,
    pub max: f32,
}

/// A font file loaded into HarfBuzz (blob + face + font).
#[derive(Debug)]
pub struct Font {
    blob: NonNull<ffi::hb_blob_t>,
    face: NonNull<ffi::hb_face_t>,
    font: NonNull<ffi::hb_font_t>,
}

impl Font {
    /// Loads face 0 of `path`. Refuses files HarfBuzz sees no glyphs in
    /// (not a font, or unreadable).
    pub fn load(path: &Path) -> Result<Font, String> {
        let c = CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|_| format!("{}: NUL in path", path.display()))?;
        // SAFETY: c is NUL-terminated and outlives the call.
        let blob = NonNull::new(unsafe { ffi::hb_blob_create_from_file_or_fail(c.as_ptr()) })
            .ok_or_else(|| format!("{}: cannot read", path.display()))?;
        // SAFETY: blob is valid; the face keeps its own reference.
        let face = unsafe { ffi::hb_face_create(blob.as_ptr(), 0) };
        // SAFETY: hb_face_create never returns NULL (the empty face at
        // worst), and hb_font_create likewise.
        let (face, font) = unsafe {
            let face = NonNull::new_unchecked(face);
            let font = NonNull::new_unchecked(ffi::hb_font_create(face.as_ptr()));
            (face, font)
        };
        let f = Font { blob, face, font };
        if f.glyph_count() == 0 {
            return Err(format!("{}: not a font (no glyphs)", path.display()));
        }
        Ok(f)
    }

    pub fn glyph_count(&self) -> u32 {
        // SAFETY: face is valid.
        unsafe { ffi::hb_face_get_glyph_count(self.face.as_ptr()) }
    }

    pub fn units_per_em(&self) -> u32 {
        // SAFETY: face is valid.
        unsafe { ffi::hb_face_get_upem(self.face.as_ptr()) }
    }

    /// File size as HarfBuzz mapped it.
    pub fn bytes(&self) -> u32 {
        // SAFETY: blob is valid.
        unsafe { ffi::hb_blob_get_length(self.blob.as_ptr()) }
    }

    /// An OpenType `name` table string (best language), if present.
    pub fn name(&self, id: ffi::hb_ot_name_id_t) -> Option<String> {
        let mut buf = vec![0 as std::os::raw::c_char; 256];
        let mut size = buf.len() as u32;
        // SAFETY: face is valid; buf has `size` bytes and HarfBuzz writes at
        // most size-1 of them plus NUL, updating size to what it wrote.
        let len = unsafe {
            ffi::hb_ot_name_get_utf8(
                self.face.as_ptr(),
                id,
                std::ptr::null(),
                &mut size,
                buf.as_mut_ptr(),
            )
        };
        if len == 0 {
            return None;
        }
        let bytes: Vec<u8> = buf[..size as usize].iter().map(|c| *c as u8).collect();
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub fn family(&self) -> String {
        self.name(ffi::NAME_ID_FAMILY).unwrap_or_else(|| "?".into())
    }

    /// Variation axes (empty for a static font).
    pub fn axes(&self) -> Vec<Axis> {
        // SAFETY: face is valid.
        let n = unsafe { ffi::hb_ot_var_get_axis_count(self.face.as_ptr()) };
        let mut infos = vec![ffi::hb_ot_var_axis_info_t::default(); n as usize];
        let mut count = n;
        // SAFETY: infos has `count` elements; HarfBuzz fills at most that
        // many and updates count.
        unsafe {
            ffi::hb_ot_var_get_axis_infos(self.face.as_ptr(), 0, &mut count, infos.as_mut_ptr())
        };
        infos
            .into_iter()
            .take(count as usize)
            .map(|a| Axis {
                tag: String::from_utf8_lossy(&a.tag.to_be_bytes()).into_owned(),
                min: a.min_value,
                default: a.default_value,
                max: a.max_value,
            })
            .collect()
    }

    /// The glyph for `c` in the font's character map (None: not covered).
    pub fn glyph(&self, c: char) -> Option<u32> {
        let mut g = 0;
        // SAFETY: font is valid; g is a valid out pointer.
        let ok = unsafe { ffi::hb_font_get_nominal_glyph(self.font.as_ptr(), c as u32, &mut g) };
        (ok != 0).then_some(g)
    }

    /// True if every character of `sample` has a glyph.
    pub fn covers(&self, sample: &str) -> bool {
        sample.chars().all(|c| self.glyph(c).is_some())
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        // SAFETY: each object is valid and released once, font first.
        unsafe {
            ffi::hb_font_destroy(self.font.as_ptr());
            ffi::hb_face_destroy(self.face.as_ptr());
            ffi::hb_blob_destroy(self.blob.as_ptr());
        }
    }
}

/// Sample text per script, for coverage reports.
pub const ARABIC_SAMPLE: &str = "بسم الله وانا ٠١٢٣";
pub const LATIN_SAMPLE: &str = "Wana OS 0123";

/// The directory with the pinned fonts for tests: `$WANA_FONTS_DIR`, or
/// the repository's `out/fonts` (filled by `tools/fetch-fonts.sh`).
pub fn test_fonts_dir() -> std::path::PathBuf {
    if let Some(d) = std::env::var_os("WANA_FONTS_DIR") {
        return d.into();
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/fonts");
    assert!(
        dir.join("SHA256SUMS").exists(),
        "pinned fonts missing in {}: run tools/fetch-fonts.sh (or make fonts)",
        dir.display()
    );
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Font {
        let dir = test_fonts_dir();
        crate::fonts::verify_dir(&dir).expect("pinned fonts verify");
        Font::load(&dir.join(name)).unwrap()
    }

    #[test]
    fn pinned_fonts_are_what_decision_0002_says() {
        let naskh = load("NotoNaskhArabic-VF.ttf");
        assert_eq!(naskh.family(), "Noto Naskh Arabic");
        assert_eq!(naskh.units_per_em(), 1000);
        assert!(naskh.covers(ARABIC_SAMPLE), "Naskh covers Arabic");
        let tags: Vec<_> = naskh.axes().into_iter().map(|a| a.tag).collect();
        assert_eq!(tags, ["wght"]);

        let sans_ar = load("NotoSansArabic-VF.ttf");
        assert_eq!(sans_ar.family(), "Noto Sans Arabic");
        assert!(sans_ar.covers(ARABIC_SAMPLE));
        let tags: Vec<_> = sans_ar.axes().into_iter().map(|a| a.tag).collect();
        assert_eq!(tags, ["wght", "wdth"], "fvar order, as the font lists them");

        let sans = load("NotoSans-VF.ttf");
        assert_eq!(sans.family(), "Noto Sans");
        assert!(sans.covers(LATIN_SAMPLE));
        assert!(
            !sans.covers("ب"),
            "Latin font has no Arabic: fallback needed"
        );
        let wght = sans.axes().into_iter().find(|a| a.tag == "wght").unwrap();
        assert_eq!((wght.min, wght.default, wght.max), (100.0, 400.0, 900.0));
    }

    #[test]
    fn non_fonts_are_refused() {
        let p = std::env::temp_dir().join(format!("wana-text-notfont-{}", std::process::id()));
        std::fs::write(&p, b"this is not a font").unwrap();
        assert!(Font::load(&p).unwrap_err().contains("not a font"));
        std::fs::remove_file(&p).unwrap();
        assert!(Font::load(Path::new("/nonexistent/x.ttf")).is_err());
    }
}
