//! Layout: text in a width becomes lines of positioned glyphs, with the
//! caret mapping between logical offsets and screen positions.
//!
//! Per paragraph (text between `\n`):
//! 1. BiDi levels for the whole paragraph (`bidi::paragraph`);
//! 2. items: maximal ranges with one level and one font. The font is the
//!    first in the fallback chain that has the character. Neutrals (spaces,
//!    digits, punctuation) stay in the previous item's font when it has
//!    them, so "وانا 2026" does not jump to the Latin font for the digits;
//! 3. measure: each item shaped once, advances per character;
//! 4. break: greedy, after spaces; a word wider than the line is broken at
//!    a character boundary;
//! 5. each line: its pieces of items reordered visually (rule L2) and
//!    shaped again on their own (a broken word is shaped as what is on the
//!    line), trailing spaces dropped, then aligned. Start is the
//!    paragraph's start side: right for RTL.
//!
//! Units: pixels at `Style::size` (font units scaled by size / units per
//! em). y grows downwards; a line's `baseline` is from the layout top.

use crate::bidi::{self, Base, Run};
use crate::font::Font;
use crate::shape::shape;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// The paragraph's start side: left for LTR, right for RTL.
    Start,
    End,
    Center,
}

#[derive(Debug, Clone)]
pub struct Style {
    /// Font size in pixels (the em).
    pub size: f32,
    pub base: Base,
    pub align: Align,
    /// Line width to break at; `None`: one line per paragraph.
    pub width: Option<f32>,
    /// BCP 47 language for shaping (e.g. `ar`), which can pick
    /// language-specific forms.
    pub language: String,
}

/// Fonts in fallback order (first = preferred).
#[derive(Debug)]
pub struct FontSet {
    pub fonts: Vec<Font>,
}

fn neutral(c: char) -> bool {
    c.is_whitespace() || c.is_ascii_punctuation() || c.is_numeric()
}

impl FontSet {
    /// The font for `c`: the previous item's font for a neutral it has,
    /// else the first font that has it, else font 0 (it draws .notdef).
    pub fn pick(&self, c: char, prev: Option<usize>) -> usize {
        if let Some(p) = prev {
            if neutral(c) && self.fonts[p].glyph(c).is_some() {
                return p;
            }
        }
        self.fonts
            .iter()
            .position(|f| f.glyph(c).is_some())
            .unwrap_or(0)
    }

    /// (ascent, descent, line gap) in pixels at `size`: the largest over
    /// the set, so every line has the same height whatever fonts it uses.
    fn metrics(&self, size: f32) -> (f32, f32, f32) {
        let mut m = (0f32, 0f32, 0f32);
        for f in &self.fonts {
            let s = size / f.units_per_em() as f32;
            let (a, d, g) = f.extents();
            m.0 = m.0.max(a as f32 * s);
            m.1 = m.1.max(-d as f32 * s);
            m.2 = m.2.max(g as f32 * s);
        }
        m
    }
}

/// A glyph placed on a line. `x` is the pen position (left edge of its
/// advance) plus the glyph's offset; `cluster` is the byte offset in the
/// whole text of the character it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub font: usize,
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    pub cluster: usize,
    pub rtl: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Byte range of the line in the text, including trailing spaces.
    pub start: usize,
    pub end: usize,
    /// End of the visible content (trailing spaces excluded).
    pub content_end: usize,
    pub rtl: bool,
    /// Left edge of the content after alignment.
    pub x: f32,
    pub width: f32,
    pub baseline: f32,
    /// Glyphs in visual order (left to right).
    pub glyphs: Vec<Placed>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub lines: Vec<Line>,
    /// The width alignment used: `Style::width`, or the widest line.
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
    text_len: usize,
}

/// A range with one BiDi level and one font (byte offsets in the text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Item {
    start: usize,
    end: usize,
    level: u8,
    font: usize,
}

/// Lays out `text` in `fonts` with `style`.
pub fn layout(text: &str, fonts: &FontSet, style: &Style) -> Result<Layout, String> {
    if fonts.fonts.is_empty() {
        return Err("no fonts".into());
    }
    let (ascent, descent, gap) = fonts.metrics(style.size);
    let line_height = ascent + descent + gap;
    let mut raw = Vec::new();
    let mut offset = 0;
    for para in text.split('\n') {
        paragraph(para, offset, fonts, style, &mut raw)?;
        offset += para.len() + 1;
    }
    let avail = style
        .width
        .unwrap_or_else(|| raw.iter().map(|l: &Line| l.width).fold(0.0, f32::max));
    for (i, line) in raw.iter_mut().enumerate() {
        let free = avail - line.width;
        let shift = match (style.align, line.rtl) {
            (Align::Start, false) | (Align::End, true) => 0.0,
            (Align::Start, true) | (Align::End, false) => free,
            (Align::Center, _) => free / 2.0,
        };
        line.x = shift;
        line.baseline = gap / 2.0 + ascent + i as f32 * line_height;
        for g in &mut line.glyphs {
            g.x += shift;
        }
    }
    Ok(Layout {
        height: raw.len() as f32 * line_height,
        lines: raw,
        width: avail,
        line_height,
        text_len: text.len(),
    })
}

fn paragraph(
    text: &str,
    offset: usize,
    fonts: &FontSet,
    style: &Style,
    out: &mut Vec<Line>,
) -> Result<(), String> {
    let para = bidi::paragraph(text, style.base)?;
    if text.is_empty() {
        out.push(Line {
            start: offset,
            end: offset,
            content_end: offset,
            rtl: para.rtl,
            x: 0.0,
            width: 0.0,
            baseline: 0.0,
            glyphs: Vec::new(),
        });
        return Ok(());
    }
    // Items (byte offsets local to the paragraph).
    let mut items: Vec<Item> = Vec::new();
    for (ci, (bi, c)) in text.char_indices().enumerate() {
        let level = para.levels[ci];
        let prev = items.last().map(|i| i.font);
        let font = fonts.pick(c, prev);
        let end = bi + c.len_utf8();
        match items.last_mut() {
            Some(it) if it.level == level && it.font == font => it.end = end,
            _ => items.push(Item {
                start: bi,
                end,
                level,
                font,
            }),
        }
    }
    // Advances per character start (pixels).
    let mut adv = vec![0f32; text.len() + 1];
    for it in &items {
        let f = &fonts.fonts[it.font];
        let scale = style.size / f.units_per_em() as f32;
        for g in shape(
            f,
            &text[it.start..it.end],
            it.level % 2 == 1,
            &style.language,
        )? {
            adv[it.start + g.cluster as usize] += g.x_advance as f32 * scale;
        }
    }
    let width_of = |a: usize, b: usize| -> f32 { adv[a..b].iter().sum() };
    for (s, e) in break_lines(text, style.width, &width_of) {
        out.push(build_line(
            text, offset, s, e, &items, fonts, style, para.rtl,
        )?);
    }
    Ok(())
}

/// Greedy line breaking. Returns byte ranges covering the whole text;
/// trailing spaces stay on the line they follow.
fn break_lines(
    text: &str,
    width: Option<f32>,
    w: &dyn Fn(usize, usize) -> f32,
) -> Vec<(usize, usize)> {
    let Some(max) = width else {
        return vec![(0, text.len())];
    };
    // Segments: a word followed by its spaces: (start, word_end, end).
    let mut segs = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let word_end = text[i..]
            .find(char::is_whitespace)
            .map_or(text.len(), |p| i + p);
        let end = text[word_end..]
            .find(|c: char| !c.is_whitespace())
            .map_or(text.len(), |p| word_end + p);
        segs.push((i, word_end, end));
        i = end;
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let mut used = 0.0; // width of the line so far, including its spaces
    for (s, we, e) in segs {
        let word = w(s, we);
        if s > start && used + word > max {
            lines.push((start, s));
            start = s;
            used = 0.0;
        }
        if word > max {
            // A word wider than the line: break it between characters.
            let mut cut_from = s;
            let mut acc = 0.0;
            for (off, c) in text[s..we].char_indices() {
                let b = s + off;
                let cw = w(b, b + c.len_utf8());
                if b > cut_from && acc + cw > max {
                    lines.push((start, b));
                    start = b;
                    cut_from = b;
                    acc = 0.0;
                }
                acc += cw;
            }
            used = acc + w(we, e);
        } else {
            used += word + w(we, e);
        }
    }
    lines.push((start, text.len()));
    lines
}

#[allow(clippy::too_many_arguments)]
fn build_line(
    text: &str,
    offset: usize,
    start: usize,
    end: usize,
    items: &[Item],
    fonts: &FontSet,
    style: &Style,
    rtl: bool,
) -> Result<Line, String> {
    let content_end = start + text[start..end].trim_end().len();
    // The items' pieces on this line, in logical order.
    let pieces: Vec<Item> = items
        .iter()
        .filter(|it| it.end > start && it.start < content_end)
        .map(|it| Item {
            start: it.start.max(start),
            end: it.end.min(content_end),
            ..*it
        })
        .collect();
    let runs: Vec<Run> = pieces
        .iter()
        .map(|p| Run {
            start: p.start,
            end: p.end,
            level: p.level,
        })
        .collect();
    let mut glyphs = Vec::new();
    let mut pen = 0.0;
    for i in bidi::visual_order(&runs) {
        let p = pieces[i];
        let f = &fonts.fonts[p.font];
        let scale = style.size / f.units_per_em() as f32;
        let prtl = p.level % 2 == 1;
        for g in shape(f, &text[p.start..p.end], prtl, &style.language)? {
            let advance = g.x_advance as f32 * scale;
            glyphs.push(Placed {
                font: p.font,
                id: g.id,
                x: pen + g.x_offset as f32 * scale,
                y: -(g.y_offset as f32) * scale,
                advance,
                cluster: offset + p.start + g.cluster as usize,
                rtl: prtl,
            });
            pen += advance;
        }
    }
    Ok(Line {
        start: offset + start,
        end: offset + end,
        content_end: offset + content_end,
        rtl,
        x: 0.0,
        width: pen,
        baseline: 0.0,
        glyphs,
    })
}

impl Layout {
    /// The line holding logical `offset` (the last line for the end).
    pub fn line_of(&self, offset: usize) -> usize {
        self.lines
            .iter()
            .position(|l| offset >= l.start && offset < l.end.max(l.start + 1))
            .unwrap_or(self.lines.len().saturating_sub(1))
    }

    /// Left and right edge of the character at `offset` on `line`, and
    /// whether it runs RTL; None if it has no glyph there (trailing space,
    /// ligature component).
    fn extent(&self, line: &Line, offset: usize) -> Option<(f32, f32, bool)> {
        let mut e: Option<(f32, f32, bool)> = None;
        for g in line.glyphs.iter().filter(|g| g.cluster == offset) {
            // The pen position is x minus any mark offset; use the advance
            // box of glyphs that advance, the glyph position of marks.
            let (l, r) = if g.advance > 0.0 {
                (g.x, g.x + g.advance)
            } else {
                (g.x, g.x)
            };
            e = Some(match e {
                None => (l, r, g.rtl),
                Some((a, b, rtl)) => (a.min(l), b.max(r), rtl),
            });
        }
        e
    }

    /// The caret for logical `offset` (0..=text length): line index and x.
    /// Before a character, the caret is at its leading edge (left for LTR,
    /// right for RTL); at the end of a line, at the trailing edge of the
    /// last character.
    pub fn caret(&self, offset: usize) -> (usize, f32) {
        let li = self.line_of(offset);
        let line = &self.lines[li];
        if offset < line.content_end {
            if let Some((l, r, rtl)) = self.extent(line, offset) {
                return (li, if rtl { r } else { l });
            }
        }
        // End of the content (or a character without its own glyph): the
        // trailing edge of the nearest earlier character on the line.
        let mut o = offset.min(line.content_end);
        while o > line.start {
            o -= 1;
            if let Some((l, r, rtl)) = self.extent(line, o) {
                return (li, if rtl { l } else { r });
            }
        }
        // Empty line: its start side.
        (
            li,
            if line.rtl {
                line.x + line.width
            } else {
                line.x
            },
        )
    }

    /// The logical offset whose caret is nearest to `x` on `line` (a click).
    pub fn hit(&self, line: usize, x: f32) -> usize {
        let l = &self.lines[line.min(self.lines.len() - 1)];
        let mut best = (f32::INFINITY, l.start);
        let mut o = l.start;
        loop {
            let (cl, cx) = self.caret(o);
            if cl == line && (cx - x).abs() < best.0 {
                best = ((cx - x).abs(), o);
            }
            if o >= l.content_end.min(self.text_len) {
                break;
            }
            o += 1;
        }
        best.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::test_fonts_dir;

    fn fonts() -> FontSet {
        let dir = test_fonts_dir();
        crate::fonts::verify_dir(&dir).expect("pinned fonts verify");
        FontSet {
            fonts: ["NotoSans-VF.ttf", "NotoSansArabic-VF.ttf"]
                .iter()
                .map(|n| Font::load(&dir.join(n)).unwrap())
                .collect(),
        }
    }

    fn style(base: Base, width: Option<f32>) -> Style {
        Style {
            size: 20.0,
            base,
            align: Align::Start,
            width,
            language: "ar".into(),
        }
    }

    /// Every char boundary of the text.
    fn boundaries(text: &str) -> Vec<usize> {
        let mut v: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
        v.push(text.len());
        v
    }

    #[test]
    fn latin_line_left_to_right() {
        let f = fonts();
        let l = layout("Wana OS", &f, &style(Base::Auto, None)).unwrap();
        assert_eq!(l.lines.len(), 1);
        let line = &l.lines[0];
        assert!(!line.rtl && line.x == 0.0);
        assert!(line.glyphs.iter().all(|g| g.font == 0), "Noto Sans");
        let clusters: Vec<usize> = line.glyphs.iter().map(|g| g.cluster).collect();
        assert_eq!(clusters, [0, 1, 2, 3, 4, 5, 6]);
        let xs: Vec<f32> = line.glyphs.iter().map(|g| g.x).collect();
        assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
        assert!((line.width - line.glyphs.iter().map(|g| g.advance).sum::<f32>()).abs() < 1e-3);
    }

    #[test]
    fn arabic_is_right_aligned_and_reads_right_to_left() {
        let f = fonts();
        let text = "مرحبا بك";
        let l = layout(text, &f, &style(Base::Auto, Some(400.0))).unwrap();
        let line = &l.lines[0];
        assert!(line.rtl);
        assert!(line.glyphs.iter().all(|g| g.font == 1), "Arabic font");
        assert!(
            (line.x + line.width - 400.0).abs() < 1e-3,
            "start = right edge"
        );
        // The first word is at the right, the second at the left.
        let first = line.glyphs.iter().find(|g| g.cluster == 0).unwrap().x;
        let second = line.glyphs.iter().find(|g| g.cluster == 11).unwrap().x;
        assert!(first > second);
    }

    #[test]
    fn mixed_line_uses_both_fonts_in_visual_order() {
        let f = fonts();
        let text = "Wana 2026 وانا";
        let l = layout(text, &f, &style(Base::Rtl, None)).unwrap();
        let line = &l.lines[0];
        // Left to right: the Arabic word (font 1), then "Wana 2026" (font 0).
        let fonts_seen: Vec<usize> = line.glyphs.iter().map(|g| g.font).collect();
        let first_latin = fonts_seen.iter().position(|&f| f == 0).unwrap();
        assert!(fonts_seen[..first_latin].iter().all(|&f| f == 1));
        assert!(fonts_seen[first_latin..].iter().all(|&f| f == 0));
        let w = line.glyphs.iter().find(|g| g.cluster == 0).unwrap();
        let waw = line.glyphs.iter().find(|g| g.cluster == 10).unwrap();
        assert!(waw.x < w.x, "the Arabic word is left of Wana");
    }

    #[test]
    fn digits_stay_with_the_arabic_font() {
        let f = fonts();
        let text = "سنة 2026";
        let l = layout(text, &f, &style(Base::Auto, None)).unwrap();
        let digits: Vec<&Placed> = l.lines[0]
            .glyphs
            .iter()
            .filter(|g| g.cluster >= 7)
            .collect();
        assert_eq!(digits.len(), 4);
        assert!(digits.iter().all(|g| g.font == 1), "no jump to Noto Sans");
        // Numbers read left to right inside the RTL line.
        let xs: Vec<(usize, f32)> = digits.iter().map(|g| (g.cluster, g.x)).collect();
        assert!(
            xs.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1),
            "{xs:?}"
        );
    }

    #[test]
    fn lines_break_between_words() {
        let f = fonts();
        let text = "مرحبا بك في وانا";
        let one = layout(text, &f, &style(Base::Auto, None)).unwrap();
        let full = one.lines[0].width;
        let l = layout(text, &f, &style(Base::Auto, Some(full * 0.6))).unwrap();
        assert_eq!(l.lines.len(), 2);
        let starts: Vec<usize> = l.lines.iter().map(|l| l.start).collect();
        assert_eq!(starts[0], 0);
        assert!(text[..starts[1]].ends_with(' '), "break after a space");
        for line in &l.lines {
            assert!(line.width <= full * 0.6 + 1e-3);
            assert!(
                (line.x + line.width - full * 0.6).abs() < 1e-3,
                "right aligned"
            );
            assert!(line.baseline > 0.0);
        }
        assert!((l.lines[1].baseline - l.lines[0].baseline - l.line_height).abs() < 1e-3);
        assert!((l.height - 2.0 * l.line_height).abs() < 1e-3);
    }

    #[test]
    fn a_word_wider_than_the_line_is_broken() {
        let f = fonts();
        let text = "Wanawanawanawana";
        let l = layout(text, &f, &style(Base::Ltr, Some(60.0))).unwrap();
        assert!(l.lines.len() > 1);
        for line in &l.lines {
            assert!(line.width <= 60.0 + 1e-3, "{}", line.width);
            assert!(!line.glyphs.is_empty());
        }
        let covered: usize = l.lines.iter().map(|l| l.end - l.start).sum();
        assert_eq!(covered, text.len(), "every character on some line");
    }

    #[test]
    fn paragraphs_split_at_newlines() {
        let f = fonts();
        let l = layout("Wana\nوانا", &f, &style(Base::Auto, None)).unwrap();
        assert_eq!(l.lines.len(), 2);
        assert!(!l.lines[0].rtl && l.lines[1].rtl, "direction per paragraph");
        assert_eq!((l.lines[1].start, l.lines[1].end), (5, 13));
    }

    #[test]
    fn alignment_end_and_center() {
        let f = fonts();
        let mut st = style(Base::Ltr, Some(300.0));
        st.align = Align::End;
        let l = layout("Wana", &f, &st).unwrap();
        assert!((l.lines[0].x + l.lines[0].width - 300.0).abs() < 1e-3);
        st.align = Align::Center;
        let l = layout("Wana", &f, &st).unwrap();
        assert!((2.0 * l.lines[0].x + l.lines[0].width - 300.0).abs() < 1e-3);
    }

    #[test]
    fn caret_moves_with_the_text_direction() {
        let f = fonts();
        for (text, rtl) in [("Wana", false), ("مرحبا", true)] {
            let l = layout(text, &f, &style(Base::Auto, Some(300.0))).unwrap();
            let xs: Vec<f32> = boundaries(text).iter().map(|&o| l.caret(o).1).collect();
            let line = &l.lines[0];
            let (left, right) = (line.x, line.x + line.width);
            if rtl {
                assert!((xs[0] - right).abs() < 1e-3, "RTL starts at the right");
                assert!((xs[xs.len() - 1] - left).abs() < 1e-3);
                assert!(xs.windows(2).all(|w| w[0] > w[1]), "{xs:?}");
            } else {
                assert!((xs[0] - left).abs() < 1e-3);
                assert!((xs[xs.len() - 1] - right).abs() < 1e-3);
                assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
            }
        }
    }

    #[test]
    fn clicking_a_caret_position_finds_its_offset() {
        let f = fonts();
        let text = "Wana 2026 وانا";
        let l = layout(text, &f, &style(Base::Rtl, None)).unwrap();
        // Positions strictly inside a run map back exactly; the boundary
        // between the runs has two logical offsets at different x
        // (bidirectional caret), each of which must also round-trip.
        for o in boundaries(text) {
            let (li, x) = l.caret(o);
            let back = l.hit(li, x);
            let (_, bx) = l.caret(back);
            assert!(
                (bx - x).abs() < 1e-3,
                "offset {o}: caret {x}, hit {back} at {bx}"
            );
        }
    }

    #[test]
    fn golden_mixed_line() {
        // A fixed layout as data: font, glyph and x (0.1 px) for each glyph.
        // Guards the whole pipeline (itemizing, shaping, L2, positions).
        let f = fonts();
        let l = layout("Wana 2026 وانا", &f, &style(Base::Rtl, None)).unwrap();
        let got: Vec<String> = l.lines[0]
            .glyphs
            .iter()
            .map(|g| format!("{}:{}@{:.1}", g.font, g.id, g.x))
            .collect();
        let got = got.join(" ");
        assert_eq!(got, GOLDEN, "update GOLDEN only after checking the change");
    }

    /// Checked by hand when recorded. Left to right:
    /// - "وانا" in Noto Sans Arabic: final alef 9, initial noon 19 with its
    ///   dot 283 (a mark: its x is its own position), isolated alef 8 (waw
    ///   does not join to its left), waw 98;
    /// - the space before it (level 1, Noto Sans: it follows the digits);
    /// - "Wana 2026" in Noto Sans: W a n a, space, 2 0 2 6.
    const GOLDEN: &str = "1:9@0.0 1:283@6.9 1:19@5.8 1:8@11.2 1:98@16.0 0:3@24.9 0:58@30.1 \
        0:68@48.3 0:81@59.5 0:68@71.9 0:3@83.1 0:21@88.3 0:19@99.7 0:21@111.2 0:25@122.6";
}
