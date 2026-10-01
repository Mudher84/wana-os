//! BiDi (UAX #9): embedding levels come from FriBidi, which resolves the
//! whole algorithm (strong/weak/neutral types, numbers, brackets, rules P2 to
//! I2). Runs and their visual order (rule L2) are computed here, so layout
//! can reorder runs per line after line breaking.
//!
//! Levels: even = LTR, odd = RTL. An LTR paragraph starts at 0, an RTL one
//! at 1. For example, a Latin word in an Arabic paragraph is at level 2.

use crate::ffi;

/// Requested paragraph direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// From the first strong character (rules P2, P3); LTR if none.
    Auto,
    Ltr,
    Rtl,
}

/// A paragraph's resolved direction and one level per character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paragraph {
    pub rtl: bool,
    pub levels: Vec<u8>,
}

/// Resolves the embedding levels of one paragraph (no line breaks inside).
pub fn paragraph(text: &str, base: Base) -> Result<Paragraph, String> {
    let chars: Vec<u32> = text.chars().map(u32::from).collect();
    let n = chars.len();
    if n == 0 {
        return Ok(Paragraph {
            rtl: base == Base::Rtl,
            levels: Vec::new(),
        });
    }
    let len = ffi::FriBidiStrIndex::try_from(n).map_err(|_| "paragraph too long".to_string())?;
    let mut types = vec![0; n];
    let mut brackets = vec![0; n];
    let mut levels = vec![0i8; n];
    let mut dir = match base {
        Base::Auto => ffi::FRIBIDI_PAR_ON,
        Base::Ltr => ffi::FRIBIDI_PAR_LTR,
        Base::Rtl => ffi::FRIBIDI_PAR_RTL,
    };
    // SAFETY: every array has n elements, n == len; FriBidi writes n
    // entries into each output array.
    let max = unsafe {
        ffi::fribidi_get_bidi_types(chars.as_ptr(), len, types.as_mut_ptr());
        ffi::fribidi_get_bracket_types(chars.as_ptr(), len, types.as_ptr(), brackets.as_mut_ptr());
        ffi::fribidi_get_par_embedding_levels_ex(
            types.as_ptr(),
            brackets.as_ptr(),
            len,
            &mut dir,
            levels.as_mut_ptr(),
        )
    };
    if max <= 0 {
        return Err("fribidi_get_par_embedding_levels_ex failed".into());
    }
    Ok(Paragraph {
        rtl: dir == ffi::FRIBIDI_PAR_RTL,
        levels: levels.into_iter().map(|l| l as u8).collect(),
    })
}

/// A maximal sequence of characters at one level: `start..end` in
/// character indices (logical order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub start: usize,
    pub end: usize,
    pub level: u8,
}

impl Run {
    pub fn rtl(&self) -> bool {
        self.level % 2 == 1
    }
}

/// Splits levels into runs, in logical order.
pub fn runs(levels: &[u8]) -> Vec<Run> {
    let mut out: Vec<Run> = Vec::new();
    for (i, &l) in levels.iter().enumerate() {
        match out.last_mut() {
            Some(r) if r.level == l => r.end = i + 1,
            _ => out.push(Run {
                start: i,
                end: i + 1,
                level: l,
            }),
        }
    }
    out
}

/// Rule L2: the order to display `runs` left to right, as indices into
/// `runs`. From the highest level down to the lowest odd level, every
/// maximal sequence of runs at that level or higher is reversed.
pub fn visual_order(runs: &[Run]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..runs.len()).collect();
    let Some(max) = runs.iter().map(|r| r.level).max() else {
        return order;
    };
    let Some(lowest_odd) = runs.iter().map(|r| r.level).filter(|l| l % 2 == 1).min() else {
        return order;
    };
    for level in (lowest_odd..=max).rev() {
        let mut i = 0;
        while i < order.len() {
            if runs[order[i]].level >= level {
                let start = i;
                while i < order.len() && runs[order[i]].level >= level {
                    i += 1;
                }
                order[start..i].reverse();
            } else {
                i += 1;
            }
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_and_numbers_inside_an_arabic_paragraph() {
        // "Wana 2026 وانا" in an RTL paragraph (a UI in Arabic).
        let text = "Wana 2026 وانا";
        let p = paragraph(text, Base::Rtl).unwrap();
        assert!(p.rtl);
        // "Wana 2026" (the space between L and EN resolves to L, W7) is at
        // level 2; the space before the Arabic word and the word are at 1.
        let mut want = vec![2u8; 9];
        want.extend([1u8; 5]);
        assert_eq!(p.levels, want);
        let r = runs(&p.levels);
        assert_eq!(r.len(), 2);
        // Displayed left to right: the Arabic run first (at the left, read
        // RTL), then "Wana 2026" at the right, where the paragraph starts.
        assert_eq!(visual_order(&r), [1, 0]);
        assert!(r[1].rtl() && !r[0].rtl());
    }

    #[test]
    fn automatic_direction_follows_the_first_strong_character() {
        let p = paragraph("Wana 2026 وانا", Base::Auto).unwrap();
        assert!(!p.rtl, "W is the first strong character");
        let mut want = vec![0u8; 10];
        want.extend([1u8; 4]);
        assert_eq!(p.levels, want);
        assert_eq!(visual_order(&runs(&p.levels)), [0, 1]);

        let p = paragraph("العام 2026", Base::Auto).unwrap();
        assert!(p.rtl, "Arabic first");
        // European digits after Arabic become Arabic numbers (W2), level 2:
        // they still read left to right inside the RTL line.
        assert_eq!(p.levels, [1, 1, 1, 1, 1, 1, 2, 2, 2, 2]);
    }

    #[test]
    fn arabic_indic_digits_read_left_to_right() {
        let p = paragraph("سنة ٢٠٢٦", Base::Auto).unwrap();
        assert!(p.rtl);
        assert_eq!(&p.levels[4..], [2, 2, 2, 2]);
    }

    #[test]
    fn brackets_follow_rule_n0() {
        // LTR paragraph, Arabic inside the brackets. Before "(" the context
        // is "a" (L), so the brackets stay LTR: only the letter is RTL.
        let p = paragraph("a (ب) c", Base::Ltr).unwrap();
        assert_eq!(p.levels, [0, 0, 0, 1, 0, 0, 0]);
        // Same paragraph, but the context before "(" is Arabic too: the
        // bracket pair takes the direction of its content and context (R),
        // so "(ج)" displays as one RTL unit with the word before it.
        let p = paragraph("ب (ج) c", Base::Ltr).unwrap();
        assert_eq!(p.levels, [1, 1, 1, 1, 1, 0, 0]);
    }

    #[test]
    fn rule_l2_reverses_nested_levels() {
        let lv = [0, 0, 1, 1, 2, 2, 1, 0];
        let r = runs(&lv);
        assert_eq!(
            r.iter().map(|r| r.level).collect::<Vec<_>>(),
            [0, 1, 2, 1, 0]
        );
        // Level 2 alone is one run; then [1, 2, 1] is reversed as a block.
        assert_eq!(visual_order(&r), [0, 3, 2, 1, 4]);
        assert_eq!(visual_order(&runs(&[0, 0])), [0], "LTR only");
        assert_eq!(visual_order(&[]), Vec::<usize>::new());
    }

    #[test]
    fn empty_paragraph() {
        let p = paragraph("", Base::Rtl).unwrap();
        assert!(p.rtl && p.levels.is_empty());
    }
}
