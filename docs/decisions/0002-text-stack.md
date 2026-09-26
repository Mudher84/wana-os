# Decision 0002: Text stack (shaping, BiDi, fonts) for the shell and Wana apps

- Status: **Accepted** (2026-09-26, by the project owner): option B, FreeType + HarfBuzz + FriBidi through Wana's
  own FFI (`wana-text`), with layout and drawing in Rust
- Date: 2026-09-26
- Context: Phase 11 (desktop shell MVP) is the first Wana component that draws text: labels, menus, the
  launcher's search field, the clock. Arabic is a primary language for Wana, not an afterthought, so the text stack
  must handle it correctly from its first line:
  - contextual letter forms (initial, medial, final and isolated) and ligatures such as lam-alef;
  - right-to-left paragraphs that mix in left-to-right runs (numbers, Latin names), which is UAX #9, the Unicode
    Bidirectional Algorithm;
  - cursor movement and selection in logical order while the text is displayed in visual order.

  This decision chooses the libraries and the division of work before any code is written, as decision 0001 did
  for the protocol layer.

## What is being chosen

Drawing a string correctly takes five jobs:

1. **Font files and selection:** which fonts exist, and which one covers a given script.
2. **BiDi:** splitting a paragraph into directional runs and ordering them visually (UAX #9).
3. **Shaping:** turning each run's characters into positioned glyphs, including Arabic joining, ligatures and
   marks. This needs the font's OpenType GSUB/GPOS tables.
4. **Rasterization:** turning glyph outlines into coverage bitmaps (antialiased).
5. **Layout and drawing:** line breaking, alignment (start/end, not left/right), a glyph atlas, and putting pixels
   on the screen.

Jobs 2 to 4 are hard, standardized and security-sensitive (fonts are untrusted input). Job 5 is Wana's product
and determines how Wana looks.

## Facts (Buildroot 2026.02.3, as pinned)

| Package | Version | Role |
|---|---|---|
| `freetype` | 2.14.3 | rasterization (job 4), font file parsing |
| `harfbuzz` | 12.3.2 | shaping (job 3); uses FreeType for font loading when enabled |
| `libfribidi` | 1.0.16 | UAX #9 BiDi (job 2) |
| `fontconfig` | 2.17.1 | font discovery and fallback (job 1) |
| `pango` / `cairo` | 1.56.4 / 1.18.4 | complete layout + 2D drawing stack (jobs 1-5), needs glib2 |
| `icu` | 78.2 | BiDi, line breaking, much more; large data file |
| `googlefontdirectory` | commit `2125bc9b…` | installs chosen fonts from google/fonts at a pinned commit (reproducible) |
| `dejavu`, `liberation` | 2.37 / 2.1.5 | general-purpose Latin fonts |

Every library in the table is already packaged, so no option needs a new Buildroot package for its libraries.

## Options

### A. Pango + Cairo (the GTK stack)

The shell calls Pango for layout and Cairo for drawing. Everything works on day one, including Arabic.

- It brings in glib2 and the GObject type system, with Pango's layout model and Cairo's drawing model. Wana's
  look and its GPU path (GLES through wana-render) would be shaped by Cairo's software-first renderer.
- Our code would be glue. The part that makes Wana look like Wana (job 5) would belong to Pango.

### B. FreeType + HarfBuzz + FriBidi through our own FFI; layout and drawing in Rust (recommended)

A new crate, `wana-text`, calls the three C libraries through small FFI declarations, as `wana-input` and
`wana-wayland` already do. It owns job 5.

- Jobs 2 to 4 use the reference implementations that every Linux desktop, Android and the browsers rely on. We do
  not reimplement Arabic shaping or UAX #9.
- Layout is ours:
  - paragraphs split into BiDi runs, each run shaped by HarfBuzz;
  - lines broken at break opportunities (spaces, and between runs), then reordered for display;
  - alignment by start/end, so an Arabic UI is right-aligned without special cases.
- Drawing is ours: a glyph atlas texture in GLES, drawn by the same compositing path as windows. The same crate
  can draw into wl_shm memory for clients that do not use GL.
- Fonts come from a fixed, pinned set (below). Discovery through fontconfig waits until third-party apps need it:
  GTK and Qt apps bring their own and read `/etc/fonts`, so installing fontconfig is a separate, later step.
- Cost: more code (layout and atlas), and cursor/selection logic in mixed-direction text is subtle. It is testable
  without a screen: logical to visual maps, glyph IDs and positions are plain data.

### C. Pure Rust (own shaper and BiDi)

Consistent with "no crates", but reimplementing OpenType shaping for Arabic (GSUB lookups, mark positioning,
joining) and UAX #9 is years of work, and a bug here renders the language wrong. Rejected.

### D. ICU instead of FriBidi

ICU also provides UAX #14 line breaking. But it is large (its data file alone is tens of MB unless trimmed), and
its C++ API needs a C wrapper. FriBidi covers what the shell needs. ICU can be added later if proper UAX #14
breaking is needed for scripts without spaces. Not now.

## Fonts (proposed)

- Arabic: **Noto Naskh Arabic** (text) and **Noto Sans Arabic** (UI).
- Latin, digits and symbols: **Noto Sans**.
- All three are OFL-1.1 and taken from google/fonts at the commit Buildroot's `googlefontdirectory` pins.
- DejaVu Sans is the fallback for anything else.
- Step 1 decided how the fonts reach the image:
  - `googlefontdirectory` downloads the whole google/fonts repository archive to install a few fonts, which is far
    too much for CI and the download cache;
  - keeping the fonts in git is ruled out by the repository's 1 MiB file limit (Noto Sans alone is 2.4 MB);
  - so a small package, `wana-fonts`, downloads only the three files from `raw.githubusercontent.com` at the
    pinned commit, with their SHA-256 in `wana-fonts.hash`.

## Recommendation

**Option B.** It matches decision 0001: proven C libraries for standardized, security-sensitive formats, reached
through our own FFI, with the product logic in Wana's Rust. Pango/Cairo (A) would be faster to a first label, but
it hands the layout and drawing model to another project and pulls glib2 into the core.

## Security

Fonts are untrusted input: a malformed font has been an exploit path before. In the shell the fonts are only the
pinned files shipped in the image, not user-supplied ones. User-installed fonts come with fontconfig and apps
later, and each app parses them in its own process.

## Phase plan (each step with its own test)

1. **Packages and fonts:**
   - Buildroot: `freetype`, `harfbuzz` (with FreeType), `libfribidi`, and `googlefontdirectory` with the chosen
     fonts;
   - `wana-text` FFI;
   - test: the fonts load, and their names, glyph counts and file hashes are logged.
2. **Shaping + BiDi as data:** `wana-text` returns runs with glyph IDs and positions. Tests, on the host and in CI:
   - "مرحبا" gets different glyph IDs for the initial, medial and final forms of the same letters than for the
     isolated letters;
   - lam-alef ("لا") takes its ligature forms, not an ordinary initial lam next to an ordinary final alef;
   - "Wana 2026 وانا" in an RTL paragraph gives the visual run order and levels from UAX #9, including the
     numbers.
3. **Layout:**
   - line breaking in a width, start/end alignment, logical to visual cursor mapping;
   - test: golden layouts, checked as data.
4. **Drawing:**
   - glyph atlas (GLES) and wl_shm rendering;
   - test: a known Arabic + Latin line rendered in QEMU, with a screenshot check of where the text is and a hash
     of the rendered bitmap (deterministic with pinned FreeType and fonts).
5. **Report + reproducibility re-check.**

Phase 11 (the shell) then starts with a working, tested text stack.
