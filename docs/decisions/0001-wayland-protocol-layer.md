# Decision 0001: Wayland protocol layer for the compositor (Phase 10)

- Status: **Accepted** (2026-09-25, by the project owner): option C, `libwayland-server` through Wana's own FFI
  and in-tree protocol generator
- Date: 2026-09-25
- Context: [ARCHITECTURE.md §7](../ARCHITECTURE.md) says the client protocol is Wayland, the implementation is our own
  Rust code, and the choice between "the Smithay protocol library" and "bare `wayland-server`" is made at the start
  of Phase 10 with written reasons. This document is that decision.

## What is being chosen

A Wayland compositor has two halves:

1. **The protocol layer.** It accepts client connections on a Unix socket, decodes requests and encodes events
   (the wire format, including file-descriptor passing), and tracks object IDs and lifetimes.
2. **The compositor logic.** Surfaces, buffers, focus, stacking, input routing, rendering and output. This is Wana's
   product, and it is ours in every option below.

The choice is who provides half 1, and whether they also take over half 2.

| Option | Protocol layer | Compositor logic |
|---|---|---|
| **A. Smithay** | `wayland-server` (Rust) | Smithay framework: its DRM/GBM/EGL/libinput/udev backends, renderer traits, `calloop` event loop, `desktop` helpers |
| **B. `wayland-server` crate** (wayland-rs, pure-Rust backend) | Rust crate from the Smithay org | Ours, on Wana's existing crates |
| **C. `libwayland-server`** (freedesktop C reference library) | C library through our own FFI, plus an in-tree protocol generator | Ours, on Wana's existing crates |

"Bare wayland-server" can mean B or C, so both are evaluated.

## Facts (measured 2026-09-25)

**Buildroot and crates.** Buildroot runs `cargo vendor` in a package's *download* step (`package/pkg-cargo.mk`,
`_DOWNLOAD_POST_PROCESS = cargo`). Wana's crates are `SITE_METHOD = local` packages. Those have no download step:
the source is rsynced (`pkg-generic.mk`), and `cargo build` runs with `--offline`. A crates.io dependency
therefore means **committing vendored sources to this repository**, plus a `.cargo/config.toml` source
replacement. Today the workspace has **zero** crates.io dependencies.

**Dependency footprint.** Resolved with `cargo tree -e normal,build` and `cargo vendor` against crates.io:

| | A: Smithay 0.7.0, our feature set¹ | A': Smithay 0.7.0, frontend only² | B: wayland-server 0.31.14 | C: libwayland-server |
|---|---|---|---|---|
| Crates to vendor | 88 | 72 | 18 | **0** |
| Vendored source, Linux-relevant | 119 MB (2.28 M lines of Rust) | (not measured separately) | 34 MB | 0. The C library is a Buildroot package (`wayland` 1.24.0, hash-pinned) |
| Vendored source, total (`cargo vendor` copies Windows crates too) | 282 MB | | 53 MB | 0 |
| Of which the library itself | smithay: 97,259 lines of Rust | | | |
| New target packages | libseat (+ what the crates dlopen or link) | | none | `wayland` (+ `expat`, `libffi`, `libxml2`; `host-wayland` for the scanner) |
| Licenses | MIT throughout (compatible) | | MIT | MIT |
| Declared MSRV | 1.80.1 (we have 1.88) | | 1.71 | n/a |

¹ `backend_drm, backend_gbm, backend_egl, backend_libinput, backend_udev, backend_session_libseat, renderer_gl,
wayland_frontend, desktop`. ² `wayland_frontend, desktop` only.

**API surface of C.**
- `wayland-server-core.h` (1.22 on the host) exports about 100 functions. A compositor uses about 50 of them:
  display, event loop, client, resource, global, signal and listener.
- The protocols needed first:
  - core `wayland.xml`: 22 interfaces, 126 requests and events;
  - `xdg-shell` (stable): 5 interfaces, 45 messages.

**Release pattern of Smithay.**
- Releases: 0.3.0 in 2021-07, then nothing until 0.4.0 in 2025-01.
- Between 2025-01 and 2025-06 came 0.4, 0.5, 0.6 and 0.7. Each 0.x step is semver-breaking.
- Since 0.7.0 (2025-06-24) there have been **451 commits** on the main branch and no release (checked 2026-09-25).
- The two largest users pin git revisions, not releases:
  - niri: `git = ".../smithay.git", rev = "79bbed5"`;
  - cosmic-comp: `[patch.crates-io] smithay = { git = ..., rev = "e3d461a" }`.
- In practice, "using Smithay" means tracking an unreleased git revision.

**Maturity of C.**
- libwayland-server is the reference implementation, ABI-stable since 1.0 (2012).
- GNOME Mutter, KDE KWin, wlroots-based compositors and Weston all use it.
- Buildroot already packages and updates it, and the image will need `libwayland-client` anyway for ordinary
  applications (GTK, Qt, SDL).

## Evaluation

### Maintenance scope

- **A (Smithay):** the least code to write at first, and the most to keep up with.
  - Every Smithay update is a breaking API change. Real users follow git, not releases.
  - Smithay brings its own DRM, GBM, EGL, libinput, udev and session backends and its own renderer abstraction.
    These overlap `wana-drm`, `wana-render` and `wana-input` (Phases 7-9). We would either throw those away or keep
    two implementations of the same layers.
  - Its `calloop` event loop, delegate macros and state traits shape the compositor's architecture.
- **B (`wayland-server` crate):** a moderate amount of code to write.
  - The typed protocol dispatch is generated for us, and Wana writes all compositor logic.
  - The crate is maintained by the same org as Smithay. Its 0.31 line has been API-stable for years, but it is
    still a third-party Rust API to follow.
  - 18 crates to vendor, review and update.
- **C (`libwayland-server`):** the most code to write at first, and the least external change to follow.
  - We write the FFI to the core (about 50 functions, in one module, like `wana-drm/src/sys.rs`).
  - We also write a small in-tree generator (Rust, no crates) that turns protocol XML into:
    - the `wl_interface` tables libwayland needs;
    - typed request traits and event senders for Wana.
    It will be about 1-1.5k lines and is ours to fix.
  - The library's ABI is frozen, and Buildroot handles its updates.
  - The same generator later produces client bindings for Wana's own clients: the shell and the test clients.

### Dependency footprint

C adds **no** Rust dependencies. It adds one C library that the system must ship anyway for applications. B adds 18
crates, mostly build-time: the scanner's `quick-xml`/`proc-macro2`/`quote`, and `rustix`/`linux-raw-sys`. A adds
72-88 crates and 119 MB of Linux-relevant vendored source, all of which must be reviewed on every update. It is
the first real supply-chain surface in the project.

### Sovereign OS alignment

- **C** matches how Wana already works:
  - own Rust code over the stable system interfaces that the Linux desktop shares (kernel uAPI; Mesa; libinput;
    eudev; and here libwayland);
  - each reached through a small FFI we own, pinned and hash-checked by Buildroot;
  - no framework deciding the architecture;
  - ARCHITECTURE §4 says "C only where we consume existing system libraries"; libwayland is exactly that.
- **B** keeps the architecture ours but moves the protocol core into a third-party Rust crate and ends the
  no-crates rule.
- **A** gives the compositor's structure to an external framework on an unreleased branch. That conflicts with
  "the implementation is our own Rust code" (ARCHITECTURE §7) and with reusing Phases 7-9.

### Security

- The protocol layer parses **untrusted** input from every client. In A and B that parsing is Rust (memory-safe).
  In C it is C: the same library every major Linux compositor ships, heavily fuzzed and audited, but C.
- In C, Wana's own unsafe code is limited to the FFI module and the generated tables. The compositor logic above
  it stays safe Rust.
- This is B's real advantage and the main cost of C. It is recorded here instead of hidden.

### Compatibility

All three speak the standard protocol, so clients built on `libwayland-client` (GTK, Qt, SDL, Chromium, and later
Xwayland) work with each. C is the reference implementation itself, so behaviour matches what applications are
tested against.

## Recommendation

**Option C: `libwayland-server` through Wana's own FFI and in-tree protocol generator.**

- It keeps the project's zero-crates rule, and adds no framework and no unreleased git dependency.
- It reuses `wana-drm`, `wana-render` and `wana-input` unchanged. One `wl_event_loop` (epoll) will watch the
  Wayland socket, the DRM fd and the libinput fd.
- It keeps the architecture ours while standing on the reference protocol implementation.
- The accepted cost is about 1.5-2.5k lines of FFI and generator code. That code is ours and tested like the rest.

**Fallback:** B, if the FFI and generator prove too costly in practice, or if we want Rust-side parsing of client
messages for security reasons. The compositor logic written for C carries over to B, because only the protocol
layer changes. **Not recommended:** A.

## Phase 10 plan (each step with its own test)

1. **Build:** Buildroot `wayland` and `wayland-protocols` packages. `crates/wana-wayland`: FFI to
   libwayland-server core, and the protocol generator. Unit tests: generated tables match the XML; the socket
   comes up.
2. **Globals:**
   - `wl_compositor`, `wl_shm`, `wl_output`, `wl_seat`, `xdg_wm_base`;
   - `wayland-info` (Buildroot `wayland-utils` 1.2.0) must list them from inside the image.
3. **Surfaces:** `wl_surface` + `wl_shm` buffers + `xdg_toplevel`, composited with GLES through `wana-render`, and
   page-flipped with `wana-drm`. Test: our own test client draws a known color, and the screenshot pixel check
   proves it reached the screen.
4. **Input:** pointer (cursor), keyboard focus and z-order, driven by `wana-input`. Test: QEMU-injected click and
   typing reach the focused test client (the client logs `[COMPOSITOR]`-tagged receipts).
5. Phase 10 test report, and reproducibility re-check.
