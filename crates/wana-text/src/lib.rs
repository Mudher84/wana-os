//! Wana OS text (decision 0002, Phase 10 → 11 prerequisite).
//!
//! Step 1: the pinned UI fonts. `fonts` verifies a font directory against
//! its `SHA256SUMS` (only the files shipped in the image are trusted), and
//! `font` loads a font through HarfBuzz and reports what it contains:
//! family, glyph count, units per em, variation axes, script coverage.
//!
//! Later steps add shaping (HarfBuzz), BiDi (FriBidi), layout and drawing.

pub mod ffi;
pub mod font;
pub mod fonts;
pub mod sha256;
