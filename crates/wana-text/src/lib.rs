//! Wana OS text (decision 0002, Phase 10 → 11 prerequisite).
//!
//! Step 1: the pinned UI fonts. `fonts` verifies a font directory against
//! its `SHA256SUMS` (only the files shipped in the image are trusted), and
//! `font` loads a font through HarfBuzz and reports what it contains:
//! family, glyph count, units per em, variation axes, script coverage.
//!
//! Step 2: `bidi` resolves a paragraph's embedding levels (FriBidi, UAX
//! #9) and orders runs visually (rule L2); `shape` turns a run into
//! positioned glyphs (HarfBuzz), which is where Arabic letters join.
//!
//! Step 3: `layout` breaks text into lines in a width, picks fonts from a
//! fallback chain, orders runs visually per line, aligns (start = right
//! for RTL) and maps carets between logical offsets and x.
//!
//! Drawing follows.

pub mod bidi;
pub mod ffi;
pub mod font;
pub mod fonts;
pub mod layout;
pub mod raster;
pub mod sha256;
pub mod shape;
