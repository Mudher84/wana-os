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

- Buildroot run [36247633814](https://github.com/Mudher84/wana-os/actions/runs/36247633814), commit `3c15221`:
  - every step passed, including the new "Text fonts" step (`[BOOT] graphics test: PASS (log: out/logs/text-boot.log)`);
  - all 13 expectations were found, and no `[INIT|RENDER]` warning or error appeared;
  - `wana-fonts` downloaded the three files and Buildroot checked them against `wana-fonts.hash`.
- The image's HarfBuzz 12.3.2 produces exactly the glyph IDs expected from the host's 8.3: `[9 322 16 25 29 77]`
  for "مرحبا", and lam 71 / alef 10 for "لا". So with the same pinned font, shaping does not change across those
  versions.
- `ci` run 36247634014 (99 unit tests with the pinned fonts from `make fonts`, and HarfBuzz/FriBidi from the
  runner's packages; MSRV): success.
- Result: **PASS**

## Text step 3: layout

Components (`wana-text/src/layout.rs`), for each paragraph (text between `\n`):
1. BiDi levels for the paragraph (automatic, LTR or RTL base).
2. Items: maximal ranges with one level and one font.
   - The font is the first one in the fallback chain (`FontSet`, in preference order: Noto Sans, then Noto Sans
     Arabic) that has the character.
   - Neutrals (spaces, digits, punctuation) stay in the previous item's font when it has them. So the digits of
     "سنة 2026" stay in the Arabic font instead of jumping to the Latin one.
3. Measuring: each item is shaped once, giving advances per character.
4. Line breaking: greedy, after spaces. A word wider than the line is broken between characters.
5. Per line:
   - the line's pieces of items are reordered visually (L2 from step 2);
   - each piece is shaped again on its own, so a broken word is shaped as what actually is on the line;
   - trailing spaces are left out of the width;
   - the line is aligned: Start = the paragraph's start side (right for RTL), End or Center;
   - every line has the same height (the largest ascent, descent and line gap in the set).
6. Carets:
   - `caret(offset)` puts the caret at the leading edge of the character at that offset: the left edge for LTR, the
     right edge for RTL; at the end of a line it is at the trailing edge of the last character;
   - `hit(line, x)` returns the offset whose caret is nearest, so a click lands where a caret would be drawn.
- `hb_font_extents_t` (48 bytes: 3 values and 9 reserved) was checked with the C compiler.

### T9: Unit tests (local)

- A Latin line runs left to right: clusters in order, x increasing, and the width is the sum of the advances.
- An Arabic line is right-aligned at the start and reads right to left: the first word is at the right.
- Mixed "Wana 2026 وانا" in an RTL paragraph: the Arabic word (Arabic font) is left of "Wana 2026" (Noto Sans).
- Digits after Arabic stay in the Arabic font and read left to right inside the RTL line.
- Breaking: two lines, the break after a space, every line within the width and right-aligned, baselines one line
  height apart.
- A word wider than the line is broken between characters, every line fits, and every character is on some line.
- Paragraphs: a direction per paragraph; the second paragraph's byte range is right.
- Alignment End and Center.
- Carets move right to left in Arabic (from the right edge to the left edge) and left to right in Latin.
- Clicks: for every offset of the mixed line, hitting its caret's x returns an offset with a caret at the same x.
  Where an LTR run meets an RTL one, two offsets share a place on the screen, the usual bidirectional caret.
- Golden layout: font, glyph ID and x of every glyph of the mixed line, checked by hand when recorded:
  - left to right: final alef 9, initial noon 19 with its dot 283, isolated alef 8 (waw does not join to its left),
    waw 98, the space, then "Wana 2026";
  - any change to itemizing, shaping, L2 or positioning shows up here.
- 110 tests in the workspace, on the pinned toolchain and on Rust 1.88.
- Result: **PASS**

### T10: Layout in a QEMU boot (local, the exact `make text-boot-test` expectations)

```
[RENDER] info: layout "مرحبا بك في Wana 2026" at 20px in 120px: 2 lines: "مرحبا بك في" 100.0px fonts {1} / "Wana 2026" 104.0px fonts {0}
[RENDER] info: layout checks: fits true, right-aligned true, carets right to left true, clicks round-trip true
[RENDER] info: text ok: layout (line breaks, fallback fonts, alignment, carets)
```
- All 16 expectations were found (13 from steps 1-2 and 3 new), and no `[INIT|RENDER]` warning or error appeared.
- The break falls between "في" and "Wana". Each line uses one font from the chain. Both lines are right-aligned,
  because the paragraph is Arabic, including the Latin line.
- Result: **PASS**

### T11: Buildroot image + `make text-boot-test` (steps 1-3) in CI

- Covered by the combined run of steps 1-4 (T16).

## Text step 4: drawing

Components (see the amendment to decision 0002: no FreeType):
- `wana-text/src/raster.rs`:
  - outlines come from HarfBuzz's draw API (`hb_font_draw_glyph`), whose callbacks receive plain coordinates;
  - curves are flattened within 1/8 px;
  - coverage uses signed-area accumulation, with non-zero fill for the overlapping contours of variable fonts;
  - `Canvas` (XRGB8888, the wl_shm format) blends with integer math;
  - `draw(layout)` places every glyph at its pen position on its line's baseline.
- `wana-wl-test --text [--fonts DIR]`:
  - lays out "مرحبا بك في وانا، نظام تشغيل مستقل Wana OS 2026" at 40 px in 560 px, white on a panel, and shows it in
    a 600x209 window;
  - logs the SHA-256 of the pixels (the rendering is deterministic) and a pixel whose 3x3 neighbourhood is fully
    inked, for the screenshot.
- The compositor clears the client's environment on purpose. So the font directory is an explicit `--fonts`
  argument, not a variable passed through.
- `make text-window-boot-test` (CI), plus a fourth scenario in `make wayland-host-test`, which checks the same hash
  headless on the host.
- Buildroot: `wana-compositor` selects `wana-text`, because `wana-wl-test` now draws text. `wana-text` no longer
  selects FreeType.

### T12: Rasterizer unit tests (local)

- A pixel-aligned rectangle is solid 255. Edges at half pixels give exactly `[128, 255, 255, 128]`, an area of
  3.0.
- A right triangle with legs of 8 has an area of 32 (±0.05), and its diagonal pixels are 128.
- Reversed winding gives the same mask, and two overlapping squares stay at 255 (non-zero fill).
- For 'W', 'a', 'g' and 'O' in Noto Sans at 32 px, the mask's box equals HarfBuzz's glyph extents rounded
  outwards, on all four sides. The center of the 'O' is empty, so its hole is a hole.
- Blending is exact: 128 of black over white gives `7f7f7f`.
- All of these passed on their first run. 116 tests in the workspace.
- Result: **PASS**

### T13: What the rendering looks like (local, host)

- The text above, rendered at 40 px with `cargo run -p wana-text --example render`, was inspected by eye:
  - the Arabic letters are joined, with their dots in place;
  - the lines are right-aligned and break after "تشغيل";
  - the second line reads "مستقل" at the right, then "Wana OS 2026" left to right.
- Result: **PASS**

### T14: Determinism

- Two headless runs of `wana-wl-test --text`: the same SHA-256
  `7e7cc3f09afa024bc2e3df715aae86f0cbe818d8684b7b4159196b8472342790`.
- `make wayland-host-test`: 4 of 4 scenarios, including "Arabic text window, rendering sha256 as in the image".
- Result: **PASS**

### T15: The text on screen (local, QEMU, the exact `make text-window-boot-test` expectations)

```
[COMPOSITOR] info: client: text rendered: 2 lines, 600x209, 8395 ink pixels, sha256 7e7cc3f09afa024bc2e3df715aae86f0cbe818d8684b7b4159196b8472342790
[COMPOSITOR] info: client: fully inked pixel at 324,48 (window coordinates)
[COMPOSITOR] info: window mapped: "wana-wl-test text" (org.wana.test) 600x209 at 340,295 (surface 8)
[BOOT] info: pixel (664,343) = #ffffff expected #ffffff
[BOOT] info: pixel (345,300) = #243b6b expected #243b6b
[BOOT] info: pixel (0,0) = #16213e expected #16213e
[BOOT] graphics test: PASS
```
- All 7 expectations and 3 pixels passed, and no `[INIT|COMPOSITOR|DRM|RENDER]` warning or error appeared.
- The screenshot shows the panel centered with the two Arabic lines, as in T13, composited by wana-compositor.
- In CI, the hash must be the same with the image's HarfBuzz 12.3.2: glyph outlines, positions and the rasterizer
  together.
- Result: **PASS**

### T16: Buildroot image + `make text-boot-test` and `make text-window-boot-test` (steps 1-4) in CI

- Buildroot run [36250188932](https://github.com/Mudher84/wana-os/actions/runs/36250188932), commit `82b62f6`:
  every step passed, including "Text stack" (16 expectations, with the layout of step 3) and "Text on screen".
- "Text on screen" expects the exact rendering hash
  `7e7cc3f09afa024bc2e3df715aae86f0cbe818d8684b7b4159196b8472342790` and 8395 ink pixels, and it passed. So the
  image's HarfBuzz 12.3.2 gives glyph outlines and positions that our rasterizer turns into pixels bit-identical to
  the host's (HarfBuzz 8.3). The screenshot pixels (664,343) white and (345,300) panel also matched.
- `ci` run 36250188905: 116 unit tests, MSRV, and `make wayland-host-test` with the text scenario (the same hash,
  headless): success.
- Result: **PASS**

## Shell step 1: layer-shell protocol and the shell's privilege

Decision 0003 (accepted: option A) chooses the protocol and the privilege model. This step builds both, before any
layer surface exists.

Components:
- `crates/wana-wayland/protocols/wlr-layer-shell-unstable-v1.xml`: v4, unchanged from the source, with its sha256
  in `protocols/README.md`. `build.rs` generates its tables like the others; its reference to `xdg_popup` resolves
  to the xdg-shell tables. Its destructors (`destroy`) are applied by the protocol layer.
- `wana-wayland`:
  - `create_privileged_global`: a global only privileged clients can see;
  - `add_privileged_client(fd)`: serves a client on a socket the compositor created (`wl_client_create`). Only this
    path makes a client privileged;
  - `wl_display_set_global_filter`: libwayland asks it both before advertising a global to a client and before
    letting that client bind it;
  - when a privileged client is destroyed, its mark is removed, so a later client allocated at the same address
    cannot inherit it.
- `wana-compositor`:
  - `zwlr_layer_shell_v1` v4 is created as a privileged global. `get_layer_surface` ends the client with an
    implementation error naming step 2 until layer surfaces exist;
  - `--shell PROGRAM [--shell-arg ARG]...`: the compositor creates a socketpair, keeps its end as the shell's
    privileged connection, and starts the shell with the other end as `WAYLAND_SOCKET`. The descriptor is
    close-on-exec everywhere except in the shell, cleared in `pre_exec`;
  - the log names the shell's connection, because on a socketpair the kernel reports the credentials of the pair's
    creator (the compositor), not of the shell;
  - if the shell exits it is logged. Restarting it comes with the shell itself (step 3).
- `wana-wl-test`:
  - `--expect-global NAME` / `--expect-no-global NAME`;
  - `--try-bind-hidden INTERFACE`: binds the registry name right after the last advertised one, which is what a
    client guessing a hidden global's number would do.

### T18: Privilege on the host (headless, the real libwayland)

- `make wayland-host-test`, 6 of 6. The new scenarios:
  - the shell (private connection) sees 6 globals including `zwlr_layer_shell_v1 v4`; an ordinary client on the
    public socket sees 5, without it;
  - binding the hidden global by guessing its name:
    ```
    [COMPOSITOR] info: client: binding hidden global name 6 as zwlr_layer_shell_v1 (not advertised to this client)
    wl_registry@2: error 0: invalid global zwlr_layer_shell_v1 (6)
    [COMPOSITOR] info: client: got the expected protocol error: wl_registry@2 code 0
    [COMPOSITOR] info: binds: wl_compositor 0, wl_shm 0, wl_output 0, wl_seat 0, xdg_wm_base 0, zwlr_layer_shell_v1 0
    ```
    libwayland refuses the bind itself, so the request never reaches the compositor's code: the global's bind
    count stays 0.
- Result: **PASS**

### T19: Privilege in a QEMU boot (local, the extended `make compositor-boot-test`)

- The compositor starts `wana-wl-test --expect-global zwlr_layer_shell_v1` as the shell, and `wayland-info` as an
  ordinary client.
- `wayland-info` lists exactly the five public globals (`wl_compositor`, `wl_shm`, `wl_output`, `wl_seat`,
  `xdg_wm_base`). The reject pattern `interface: 'zwlr_layer_shell_v1'` is absent.
- The shell logs `global zwlr_layer_shell_v1 v4 visible, as expected`, and the compositor logs
  `client connected: the shell (private connection)`.
- All the earlier expectations of the test still pass.
- Result: **PASS**

### T20: Buildroot image + `make compositor-boot-test` (with the shell) in CI

- Actual: *pending*

## Shell step 2: layer surfaces

Components:
- `wana-compositor/src/layer.rs`: the rules, free of libwayland and unit-tested.
  - Validation: a size of 0 needs both opposite anchors (`invalid_size`); the anchor is 4 bits (`invalid_anchor`);
    layer 0..3, keyboard interactivity 0..2.
  - The exclusive edge: a positive zone counts only when the surface is anchored to one edge, or one edge and both
    perpendicular ones.
  - Arrangement:
    - surfaces with a positive zone first, from overlay down to background, each against the area still usable,
      each taking its zone plus margin off that edge;
    - then the others in the usable area (zone 0) or on the whole output (zone -1);
    - a size of 0 stretches between the anchors minus the margins, and an unanchored axis is centered.
- `wana-compositor/src/shell_surfaces.rs`: the protocol.
  - `get_layer_surface` checks the surface's role (`role`), a buffer already attached (`already_constructed`) and
    the layer (`invalid_layer`).
  - State is double-buffered and validated at commit.
  - The handshake is xdg's: an initial commit without a buffer, then configure (serial, width, height), then ack;
    a buffer before the ack is `invalid_surface_state`.
  - A null buffer unmaps the surface and returns it to its state right after `get_layer_surface`.
  - After every layer commit the surfaces are arranged again. Each one whose size changed gets a new configure, and
    windows are placed in the area left for them.
  - A surface reserves its zone only while mapped, so windows do not move for a panel that is not on screen yet.
  - `get_popup` is refused for now with an implementation error.
- Stacking (`stack()`): background, then bottom, then windows, then top, then overlay. Drawing and input both use
  it.
- Input:
  - the pointer hit test covers layer surfaces too;
  - a click on a layer surface with keyboard interactivity `none` does not take the keyboard, and does not raise
    anything;
  - a mapped top/overlay surface with `exclusive` interactivity holds the keyboard until it unmaps. This is for the
    launcher and the lock screen.
- `surface::place` centers new windows in the usable area instead of the whole output.
- The compositor has `--exit-with-shell` for tests: it stops when the shell exits and returns the shell's result.
- `wana-wl-test`:
  - `--layers`, run as the shell: a background (all edges, zone -1) and a 40 px top bar (zone 40), each through the
    handshake, then an ordinary window;
  - `--layer-invalid-size`: width 0 anchored only to the top.

### T21: Unit tests (local)

- Validation and anchor, layer and keyboard ranges.
- The exclusive edge for every anchor case in the protocol text (one edge, one edge and both perpendicular edges, a
  corner, parallel edges, all edges, zone 0).
- Arrangement:
  - a background at -1 covers the output while the bar takes 40 px;
  - zones stack in layer order and include margins: a top bar with margin 4, a left panel below it, a bottom dock
    centered in what is left;
  - a zone 0 notification moves below the bar;
  - an unanchored launcher is centered.
- Window placement inside the usable area: 400,260 below a 40 px bar.
- All passed on their first run. 122 tests in the workspace.
- Result: **PASS**

### T22: On the host (headless) and in a QEMU boot (local, the exact `make layer-boot-test` expectations)

- `make wayland-host-test`: 8 of 8. The new scenarios:
  - background + bar, and the window below the bar;
  - width 0 without both side anchors: `zwlr_layer_surface_v1 error 1: width 0 needs anchors to both the left and
    the right edge`.
- QEMU:
  ```
  [COMPOSITOR] info: client: layer "wana-desktop" configured 1280x800 (serial 1)
  [COMPOSITOR] info: layer surface mapped: "wana-desktop" on layer background at 0,0 1280x800 (exclusive zone -1)
  [COMPOSITOR] info: client: layer "wana-bar" configured 1280x40 (serial 2)
  [COMPOSITOR] info: usable area for windows: 0,40 1280x760
  [COMPOSITOR] info: layer surface mapped: "wana-bar" on layer top at 0,0 1280x40 (exclusive zone 40)
  [COMPOSITOR] info: window mapped: "wana-wl-test" (org.wana.test) 480x320 at 400,260 (surface 17)
  [BOOT] info: pixel (640,20) = #0b0f1a expected #0b0f1a
  [BOOT] info: pixel (100,400) = #1b3a5c expected #1b3a5c
  [BOOT] info: pixel (640,250) = #1b3a5c expected #1b3a5c
  [BOOT] info: pixel (640,262) = #ffffff expected #ffffff
  [BOOT] info: pixel (640,420) = #4f8cff expected #4f8cff
  ```
  - The pixels are: the bar, then the desktop left of and above the window (not the compositor's own background),
    then the window's top border at y 262, now 20 px lower than before, then its interior.
  - When the shell exits, its layers are destroyed and the usable area goes back to the whole output.
- All 10 expectations and 5 pixels passed, and no `[INIT|COMPOSITOR|DRM|RENDER]` warning or error appeared.
- Result: **PASS**

### T23: Buildroot image + `make layer-boot-test` in CI

- Actual: *pending*

## Shell step 3a: wana-shell with the desktop and the top bar

Components:
- `crates/wana-client` (new), shared by the shell and the test client:
  - the libwayland-client binding, moved from `wana-wl-test`, plus `wait(timeout)`: the shell redraws its clock
    between events;
  - `shm::Buffer`: memfd-backed XRGB8888 buffers. The compositor copies at commit, so a buffer is destroyed right
    after the commit that uses it;
  - `layer::LayerSurface`: create, set the state, initial commit, wait for the configure, ack.
- `crates/wana-shell` (new):
  - it binds `zwlr_layer_shell_v1`, which only works on the private connection; anywhere else it fails with "not
    started as the shell";
  - the desktop is a background layer surface (all edges, zone -1) with a vertical gradient;
  - the top bar is a top layer surface, 40 px, zone 40, RTL: "وانا" at the start (right) and the time at the end
    (left) in Arabic-Indic digits, laid out and drawn with wana-text;
  - the time is UTC until time zones become configurable (Settings), and the bar is redrawn when the minute
    changes. `--clock HH:MM` fixes it for tests;
  - `draw.rs` is pure (state to pixels) and unit-tested; every rendering is logged with its SHA-256.
- Autostart (`--autostart PROGRAM [--autostart-arg ARG]... [--exit-with-autostart]`), after the shell's
  surfaces are mapped:
  - the program starts with a clean environment and the public socket (`WAYLAND_DISPLAY`, which the compositor now
    passes to the shell);
  - the shell marks its privileged descriptor close-on-exec itself, in addition to libwayland doing so, so no
    started program can hold it;
  - starting programs from the shell also removes a race: the test's window cannot map before the bar reserves its
    zone.
- `wana-wl-test --no-inherited-fds`: fails if the process received any descriptor besides 0, 1 and 2.
  - Its first version picked the wrong descriptor to ignore: it assumed the listing's own descriptor was the
    highest, but it is the lowest free one.
  - Checked with a deliberately inherited descriptor, it now reports `5 -> /dev/null` and exits 1. On a clean
    process it passes.
- Buildroot: `wana-compositor` builds and installs `/usr/bin/wana-shell`.

### T24: Unit tests (local)

- The gradient's end colors, and its interpolation rounding.
- Arabic-Indic digits (`16:20` becomes `١٦:٢٠`).
- The clock (UTC hours and minutes).
- The bar:
  - ink only at the right (the brand) and at the left (the time), none in the middle, the padding kept;
  - the same inputs give the same pixels, and another minute gives different ones.
- 126 tests in the workspace.
- Result: **PASS**

### T25: The shell on the host (headless) and in a QEMU boot (local, the exact `make shell-boot-test` expectations)

- `make wayland-host-test`: 9 of 9. The new scenario checks the desktop and bar hashes, the autostarted client
  with no inherited descriptors, and its window below the bar.
- QEMU:
  ```
  [SHELL] info: wana-shell 0.1.0 starting
  [COMPOSITOR] info: client connected: the shell (private connection)
  [SHELL] info: desktop mapped: 1280x800, sha256 c203532a708e005885a04bb854150ee8b4ed80eeef930d613d377756e9bf4839
  [SHELL] info: bar mapped: 1280x40, time ١٦:٢٠, sha256 1359d3bd16029f7e54f4b04c42f0a23df6c55d872ffea676aad584f10a39b5a8
  [COMPOSITOR] info: usable area for windows: 0,40 1280x760
  [SHELL] info: ready
  [SHELL] info: autostart: /usr/bin/wana-wl-test --no-inherited-fds --hold 3 (pid …)
  [COMPOSITOR] info: client: inherited descriptors: 0 1 2 only
  [COMPOSITOR] info: window mapped: "wana-wl-test" (org.wana.test) 480x320 at 400,260 (surface …)
  [BOOT] info: pixel (640,20) = #0b0f1a expected #0b0f1a
  [BOOT] info: pixel (640,262) = #ffffff expected #ffffff
  [BOOT] info: pixel (640,420) = #4f8cff expected #4f8cff
  ```
- All 13 expectations and 3 pixels passed, and no `[INIT|COMPOSITOR|DRM|RENDER|SHELL]` warning or error appeared.
  The screenshot shows the gradient desktop, the bar with "وانا" at the right and "١٦:٢٠" at the left, and the
  window below the bar.
- Result: **PASS**

### T26: Buildroot image + `make shell-boot-test` in CI

- Actual: *pending*

## Shell step 3b: the launcher

Components (`crates/wana-shell`):
- `apps.rs`: the apps come from a pinned file, one per line, fields separated by a TAB:
  `name<TAB>/absolute/program<TAB>argument...`.
  - A TAB separator lets names contain spaces without a quoting syntax.
  - A bad line is rejected with its number. A missing or broken file leaves the launcher empty and is logged as a
    warning; the desktop stays up.
  - The image installs `/etc/wana/apps`: two entries for now, both `wana-wl-test`, the only Wayland app so far.
  - It also installs `/usr/share/wana-shell/apps.test` for the boot test: short-lived apps that check they inherit
    no descriptor.
- `launcher.rs`: a pure state machine over evdev key codes, so navigation does not depend on the keyboard layout:
  - Up, Down, Home and End select (no wrap-around);
  - Enter or keypad Enter starts the selected app;
  - Escape closes the launcher.
- The launcher surface:
  - an overlay layer surface, unanchored, so the compositor centers it in the usable area;
  - keyboard interactivity is exclusive, so it holds the keyboard while open;
  - drawn by `draw::launcher`: a title ("التطبيقات"), then one row per app, right-aligned (RTL), with the
    selected row in the accent color.
- Opening and closing:
  - a click on the bar's start (the right 160 px, where "وانا" is) opens it, and a second click closes it;
  - a click on a row starts that app;
  - closing destroys the surface; the compositor then gives the keyboard back to the top window.
- The first frame of the launcher is logged once presented (a frame callback), with the rendering's SHA-256.
- Apps start like the autostart: through the public socket, with a clean environment.
- The shell now binds `wl_seat` and creates the pointer and keyboard when the capabilities appear. The keymap
  descriptor is closed unread, since the shell uses key codes.
- Test flags:
  - `--exit-with-launched` ends the shell with the result of the first app started from the launcher;
  - `--test-launch N` opens the launcher once ready and starts app N as soon as it is shown. It exists for the
    headless host, which has no input devices.
- `tools/qemu-graphics-test.py`: a `--send` of the form `wait:REGEX` pauses the input list until a log line
  matches. The test clicks, waits until the launcher is shown, presses Down, waits for the new selection, then
  presses Enter. Nothing depends on timing.
- The key binding to open the launcher is not in this step. The compositor would have to tell the shell about it,
  which needs a private protocol, so it comes with its own decision.

### T27: Unit tests (local)

- Parsing the apps file: names with spaces; errors that name the line; both shipped lists parse.
- The menu: moving at the ends, Enter, keypad Enter, Escape, other keys, an empty list.
- The launcher drawing:
  - border, background and accent where expected;
  - the title and the names at the right, nothing at the left of a short name;
  - another selection gives different pixels;
  - finding the row under a point.
- 132 tests in the workspace.
- Result: **PASS**

### T28: The launcher on the host (headless) and in a QEMU boot (local, the exact `make launcher-boot-test` expectations)

- `make wayland-host-test`: 10 of 10. The new scenario uses `--test-launch 2` and checks:
  - the launcher's position and hash;
  - that it takes the keyboard;
  - that it is destroyed when the app starts;
  - that the app inherits no descriptor, maps its window and exits successfully.
- QEMU: the input is sent through the QEMU monitor. Six pointer moves push the cursor to the top-right corner,
  then comes a click. After `wait:launcher shown` it sends `sendkey down`, and after `wait:launcher: selected 2/2`
  it sends `sendkey ret`.
  ```
  [SHELL] info: apps: 2 from /usr/share/wana-shell/apps.test
  [SHELL] info: launcher opened from the bar
  [COMPOSITOR] info: layer surface mapped: "wana-launcher" on layer overlay at 400,340 480x160 (exclusive zone 0)
  [COMPOSITOR] info: keyboard focus: layer "wana-launcher"
  [SHELL] info: launcher shown: 480x160, selected 1/2 "نافذة تجريبية", sha256 05aea7f6617f5b2511e1f7a1b2ed775c67f8e85d3946b43354e074b8a45cc378
  [SHELL] info: launcher: selected 2/2 "نص عربي"
  [SHELL] info: launcher closed
  [SHELL] info: app "نص عربي": /usr/bin/wana-wl-test --no-inherited-fds --text --hold 2 (pid …)
  [COMPOSITOR] info: layer surface destroyed: "wana-launcher"
  [COMPOSITOR] info: client: inherited descriptors: 0 1 2 only
  [COMPOSITOR] info: client: text rendered: 2 lines, 600x209, 8395 ink pixels, sha256 7e7cc3f09afa024bc2e3df715aae86f0cbe818d8684b7b4159196b8472342790
  [COMPOSITOR] info: keyboard focus: "wana-wl-test text" (org.wana.test)
  [SHELL] info: app "نص عربي" exited successfully
  [BOOT] info: pixel (640,20) = #0b0f1a expected #0b0f1a
  [BOOT] info: pixel (420,420) = #4f8cff expected #4f8cff
  [BOOT] info: pixel (420,468) = #1e2638 expected #1e2638
  ```
- All 19 expectations and 3 pixels passed under TCG (no KVM), and no `[INIT|COMPOSITOR|DRM|RENDER|SHELL]`
  warning or error appeared.
- The launcher's hash is the same on the host and in the image.
- The screenshot shows the panel centered below the bar: the title, then "نافذة تجريبية" selected, then
  "نص عربي".
- Result: **PASS**

### T29: Buildroot image + `make launcher-boot-test` in CI

- Actual: *pending*

## Status

- Text step 1 (pinned fonts): **PASS** (T1-T4).
- Text step 2 (shaping + BiDi): **PASS** (T5-T8).
- Text step 3 (layout): **PASS** (T9-T11).
- Text step 4 (drawing): **PASS** (T12-T16).
- Text step 5 (reproducibility re-check with the text stack in the image): pending.
- Shell step 1 (layer-shell protocol + privilege, [decision 0003](../decisions/0003-shell-surfaces.md) accepted):
  T18-T19 pass locally; T20 (CI) is pending.
- Shell step 2 (layer surfaces): T21-T22 pass locally; T23 (CI) is pending.
- Shell step 3a (wana-shell: desktop, Arabic top bar, autostart): T24-T25 pass locally; T26 (CI) is pending.
- Shell step 3b (launcher): T27-T28 pass locally; T29 (CI) is pending.
- Shell step 3c (dock via ext-foreign-toplevel-list; restarting a crashed shell) and the launcher's key binding
  (a private protocol, with its own decision): next.
