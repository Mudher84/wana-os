# Test report: Phase 11 (desktop shell MVP)

Phase 11 starts with the text stack the shell needs ([decision 0002](../decisions/0002-text-stack.md)):
1. pinned fonts in the image, verified and loaded;
2. shaping + BiDi as data;
3. layout;
4. drawing;
5. report + reproducibility re-check.

The shell itself follows once text works.

This report grows with each step.

## Text step 1: pinned fonts

Components:
- `wana-fonts` (Buildroot package):
  - downloads Noto Naskh Arabic, Noto Sans Arabic and Noto Sans (variable fonts, OFL-1.1) as three single files
    from google/fonts at commit `2125bc9b…`, the one `googlefontdirectory` pins;
  - `wana-fonts.hash` pins each file's SHA-256, and Buildroot refuses anything else;
  - installs them under plain names in `/usr/share/fonts/wana`, with a `SHA256SUMS` file and the two license
    texts.
- `tools/fetch-fonts.sh` (host and CI): the same files and hashes, read from `wana-fonts.mk` and
  `wana-fonts.hash`, fetched into `out/fonts`. Unit tests then use exactly the image's fonts. This matters because
  glyph IDs depend on the font version, so later shaping tests must not depend on whatever fonts the host has.
  `make test` and `make msrv` run it first; it is idempotent.
- `crates/wana-text`:
  - FFI to libharfbuzz: opaque handles only, plus `hb_ot_var_axis_info_t`, which was checked against the header;
  - a dependency-free SHA-256;
  - `fonts::verify_dir`: every font listed in `SHA256SUMS` must match, a changed or missing font is refused, and
    names must be plain file names;
  - `font::Font`: family name, glyph count, units per em, variation axes, per-character coverage.
- The `wana-text` tool, run by `make text-boot-test` in CI, logs what it verifies and loads. It fails unless the
  fonts together cover Arabic and Latin.

### T1: Unit tests (local)

- The SHA-256 test vectors come from FIPS 180-2 (empty input, "abc", the 56-byte message), plus lengths 55, 56 and
  64, the padding boundaries. Python's hashlib gives the same values.
- `SHA256SUMS` parsing refuses:
  - paths (`../etc/passwd`) and hidden names;
  - short or uppercase hashes;
  - empty files.
- A changed file is refused ("refusing a changed font"), and so is a missing one.
- The pinned fonts are what decision 0002 says:
  - family names;
  - 1000 units per em;
  - axes: `wght` 400..700 for Naskh, `wght` 100..900 (default 400) plus `wdth` for the Sans fonts;
  - Arabic coverage for both Arabic fonts;
  - Noto Sans has no Arabic glyph, which shows why the layout needs font fallback.
- A non-font file and a missing path are refused.
- One test was wrong at first: it expected Noto Sans Arabic's axes in the order `wdth, wght`. The font lists
  `wght, wdth`, and the test now expects that order.
- Result: **PASS** (87 tests in the workspace)

### T2: The fonts in a QEMU boot (local, the exact `make text-boot-test` expectations)

- `wana-text` as `wana.run`, with the fonts from `tools/fetch-fonts.sh` installed as the package installs them:
  ```
  [RENDER] info: font verified: NotoNaskhArabic-VF.ttf (329920 bytes, sha256 02d9310b…2931f6)
  [RENDER] info: font loaded: NotoNaskhArabic-VF.ttf: "Noto Naskh Arabic", 1726 glyphs, 1000 units/em, axes [wght 400..700 (default 400)], covers [Arabic, Latin]
  [RENDER] info: font verified: NotoSansArabic-VF.ttf (844676 bytes, sha256 63111b5b…e869e)
  [RENDER] info: font loaded: NotoSansArabic-VF.ttf: "Noto Sans Arabic", 1711 glyphs, 1000 units/em, axes [wght 100..900 (default 400), wdth 62.5..100 (default 100)], covers [Arabic, Latin]
  [RENDER] info: font verified: NotoSans-VF.ttf (2493792 bytes, sha256 e0890ec6…54717)
  [RENDER] info: font loaded: NotoSans-VF.ttf: "Noto Sans", 4671 glyphs, 1000 units/em, axes [wght 100..900 (default 400), wdth 62.5..100 (default 100)], covers [Latin]
  [RENDER] info: fonts ok: 3 verified and loaded, Arabic and Latin covered
  ```
- All 9 expectations were found, and no `[INIT|RENDER]` warning or error appeared.
- Negative cases (host):
  - one byte changed in `NotoSans-VF.ttf`: `[RENDER] error: …: sha256 61d00095…, SHA256SUMS says e0890ec6…
    (refusing a changed font)`, exit 1;
  - the file removed: `No such file or directory`, exit 1.
- Result: **PASS**

### T3: Buildroot: package download and hashes, defconfig

- `make br-wana-fonts-source`: all three files downloaded from the pinned URLs, and each is `OK (sha256: …)`
  against `wana-fonts.hash`. Buildroot handles the URL-encoded names (`%5B`, `%2C`).
- `make config-check`: the defconfig round-trips with `BR2_PACKAGE_WANA_TEXT=y`, which selects `wana-fonts`,
  `harfbuzz` and `freetype`.
- The full image build and `make text-boot-test` run in CI (T4).
- Result: **PASS** (local part)

### T4: Buildroot image + `make text-boot-test` in CI

- Covered by the combined run of steps 1 and 2 (T8).

## Text step 2: shaping and BiDi as data

Components:
- `wana-text/src/bidi.rs`:
  - one paragraph's embedding levels come from FriBidi (`fribidi_get_bidi_types`, `fribidi_get_bracket_types`,
    `fribidi_get_par_embedding_levels_ex`), that is the whole of UAX #9 from P2 to I2, including numbers and the
    bracket pairs of N0;
  - runs and their visual order are computed in Rust (rule L2), because layout reorders per line, after line
    breaking;
  - the FriBidi constants were checked by compiling against the header: `PAR_LTR` 0x110, `PAR_RTL` 0x111,
    `PAR_ON` 0x40.
- `wana-text/src/shape.rs`: one run (text, direction, language) becomes positioned glyphs through HarfBuzz
  (`hb_shape`).
  - It uses cluster level "monotone characters", so every character keeps a cluster for cursor positions later.
  - Glyphs come in visual order, so an RTL run's clusters decrease.
  - `hb_glyph_info_t` and `hb_glyph_position_t` are 20 bytes each, checked with the C compiler.
- The `wana-text` tool also shapes Arabic samples and resolves a mixed line in the boot test. It exits 1 if the
  letters do not join, if lam-alef is not formed, or if the BiDi result is wrong.

### T5: What the pinned fonts actually do (found while writing the tests)

The first shaping tests assumed two things that are true of many fonts but not of these:
- "مرحبا" has 6 glyphs, not 5. The Noto Arabic fonts draw a dotted letter as a dotless body plus its dot: a
  separate glyph with zero advance, positioned like a mark (beh: body 14/15/19 depending on position, dot 322).
- lam-alef is not a single ligature glyph. It is two dedicated glyphs, lam 71 and alef 10 in Naskh. An initial lam
  (before meem) is 70 and a final alef (after beh) is 9.

The tests now check what must hold for any correct Arabic rendering, not one font's glyph count:
- every glyph of a joined word differs from the isolated glyph in the character map;
- beh has three different bodies: isolated, initial and final;
- lam-alef uses forms that differ from an ordinary initial lam and final alef;
- isolated letters keep their nominal glyphs.

### T6: Unit tests (local)

- BiDi. All of these were written from the UAX #9 rules first, and FriBidi agreed with every one on the first run:
  - "Wana 2026 وانا" in an RTL paragraph: levels 2 for "Wana 2026" (the space between L and EN resolves to L, W7)
    and 1 for " وانا", so the display, left to right, is "وانا" | "Wana 2026";
  - the automatic direction follows the first strong character (P2, P3);
  - European digits after Arabic become Arabic numbers (W2) at level 2;
  - Arabic-Indic digits are at level 2;
  - N0 brackets: "a (ب) c" keeps its brackets LTR, while "ب (ج) c" makes them RTL;
  - L2 with nested levels [0,1,2,1,0] gives the order [0,3,2,1,4];
  - an empty paragraph.
- Shaping: the invariants of T5, a Latin run left to right with nominal glyphs, and empty text.
- 99 tests in the workspace, on the pinned toolchain and on Rust 1.88.
- Result: **PASS**

### T7: Shaping and BiDi in a QEMU boot (local, the exact `make text-boot-test` expectations)

```
[RENDER] info: shaped "مرحبا" (Noto Naskh Arabic, RTL): 6 glyphs [9 322 16 25 29 77], width 2041 units, joined forms: yes
[RENDER] info: shaped "لا": lam 71 (initial lam 70), alef 10 (final alef 9): lam-alef forms: yes
[RENDER] info: bidi "Wana 2026 وانا" (RTL paragraph): levels 22222222211111, left to right: "وانا" RTL | "Wana 2026" LTR
[RENDER] info: text ok: fonts, Arabic shaping and BiDi (FriBidi, UAX #9)
```
- All 13 expectations were found (the 9 of step 1 and 4 new), and no `[INIT|RENDER]` warning or error appeared.
  The Arabic text passes intact through the serial console and the log.
- The glyph IDs in the expectations are from the host's HarfBuzz 8.3. The image has 12.3.2, so the CI run also
  checks that shaping with the same font agrees across versions.
- Result: **PASS**

### T8: Buildroot image + `make text-boot-test` (steps 1+2) in CI

- Actual: *pending*
