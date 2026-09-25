# Test report: Phase 10 (compositor MVP)

Phase 10 is built in steps ([decision 0001](../decisions/0001-wayland-protocol-layer.md)):
1. the protocol layer and the Wayland socket;
2. globals;
3. surfaces on screen;
4. input focus and z-order.

This report grows with each step.

## Step 1: protocol layer and Wayland socket

Components:
- `crates/wana-wayland`:
  - `sys.rs`: FFI to libwayland-server (display, socket, event loop, client credentials, listeners), with the stable
    `wl_message`/`wl_interface`/`wl_list`/`wl_listener` layouts. No crates.
  - `scanner.rs`: Wana's protocol scanner. It is a minimal XML reader for protocol files (comments, entities,
    attributes) plus a code generator. It turns the protocol XML into the `wl_interface` tables libwayland needs,
    and adds opcode constants. Signature and `types` rules follow wayland-scanner's `private-code`: version
    prefix, `?` for nullable arguments, untyped `new_id` as `sun`.
  - `build.rs`: runs the scanner at build time on `wayland.xml` and `xdg-shell.xml`, taken from the same packages
    that provide the library. In Buildroot that is `$(STAGING_DIR)` via `WANA_WAYLAND_XML` and
    `WANA_WAYLAND_PROTOCOLS_DIR`; on a dev host it is `/usr/share`. The tables always match the library
    version they run with.
  - `server.rs`: `Display`, a safe wrapper for the socket (`wl_display_add_socket_auto`), the event loop
    (flush + dispatch) and client connect/disconnect events with SO_PEERCRED credentials. C callbacks only
    queue events, and the Rust side drains them; no closure is called from C.
- `crates/wana-compositor` (`/usr/bin/wana-compositor`):
  - prepares `XDG_RUNTIME_DIR`, creating `/run/user/<uid>` with mode 0700 when unset, and checking it when set;
  - opens the socket, runs the event loop and logs clients as `[COMPOSITOR]`;
  - `--run PROGRAM` serves one client and returns its result (for boot tests); `--client-debug` sets
    `WAYLAND_DEBUG=1` for it.
- Buildroot:
  - `wayland` 1.24.0 and `wayland-protocols` 1.45 (build-time XML);
  - `wayland-utils` 1.2.0 (`wayland-info`, the reference test client);
  - the new `wana-compositor` package.
  With `wayland` enabled, Buildroot's Mesa now also builds its Wayland EGL platform, which clients will need.
- `make compositor-boot-test` and a CI step.

The XML is not copied into the repository. It is read from the hash-verified Buildroot packages. freedesktop.org
is not reachable from the development sandbox, so the local tests use the host's `/usr/share` copies
(wayland-protocols 1.45, the same version as Buildroot; wayland.xml from libwayland 1.22, matching the host
library).

### T1: Unit tests (local)

- `make check`: 53 tests, of which 9 are new in `wana-wayland`.
- Scanner:
  - parses a protocol, skipping comments and decoding entities;
  - signatures and types follow wayland-scanner (`usun`, `2?oih?s`);
  - malformed XML, unknown types and unknown interface references are build errors.
- ABI: `wl_message` is 24 bytes, `wl_interface` 40, `wl_listener` 24.
- `core_tables_match_libwayland`: every generated core table equals the table libwayland-server exports under
  the same name (looked up with `dlsym`). That covers name, version, every request and event name and signature,
  and every argument interface: 22 of 22 interfaces. This checks the generator against the reference.
- `xdg_shell_references_core_tables`: 5 interfaces. `xdg_wm_base.get_xdg_surface` has signature `no`, and its
  `wl_surface` argument points at the generated core table, across protocols.
- `socket_round_trip_with_a_raw_client`: a hand-written client speaks the wire format to the socket:
  `get_registry` + `sync`. It receives `wl_callback.done` on object 3 and `wl_display.delete_id(3)`. The connect
  and the disconnect are reported with the test's pid, and the socket file is gone after the display is
  dropped.
- clippy `-D warnings` is clean, and the tests also pass on Rust 1.88 (MSRV).
- Result: **PASS**

### T2: wana-compositor serves wayland-info (local, host, then QEMU)

- On the host, and in a QEMU boot with the same expectations as `make compositor-boot-test` (TCG, initramfs,
  host libwayland 1.22):
  ```
  [COMPOSITOR] info: wana-compositor 0.1.0 starting
  [COMPOSITOR] info: protocol tables: 22 core + 5 xdg-shell interfaces (generated from XML)
  [COMPOSITOR] info: XDG_RUNTIME_DIR=/run/user/0 (created, mode 0700)
  [COMPOSITOR] info: listening on /run/user/0/wayland-0 (WAYLAND_DISPLAY=wayland-0)
  [COMPOSITOR] info: running test client /usr/bin/wayland-info (pid 99)
   -> wl_display@1.get_registry(new id wl_registry@2)
   -> wl_display@1.sync(new id wl_callback@3)
  [COMPOSITOR] info: client connected: pid 99 uid 0 gid 0
  wl_display@1.delete_id(3)
  wl_callback@3.done(0)
  [COMPOSITOR] info: client disconnected: pid 99
  [COMPOSITOR] info: test client /usr/bin/wayland-info exited successfully
  [COMPOSITOR] info: 1 client connection(s) served
  [COMPOSITOR] info: shut down; socket removed
  [INIT] info: /usr/bin/wana-compositor exited successfully
  ```
  The trace lines are the host run (`WAYLAND_DEBUG`). All 11 expectations and the `[INIT|COMPOSITOR] error`
  rejection pass in the QEMU run.
- `wayland-info` lists no interfaces yet: no globals exist until step 2.
- Result: **PASS**

### T3: Failures are reported at [COMPOSITOR] (local)

- Client fails (`--run /usr/bin/false`): `[COMPOSITOR] error: test client /usr/bin/false failed: exit status: 1`,
  exit 1. The socket is still removed.
- Client hangs (`--timeout 2 --run sleep 30`): `[COMPOSITOR] error: timeout after 2s; test client killed`, exit 1.
- Unusable runtime directory (`XDG_RUNTIME_DIR=/nonexistent/dir`):
  `[COMPOSITOR] error: XDG_RUNTIME_DIR=/nonexistent/dir: No such file or directory`, exit 1.
  - First attempt: libwayland printed `unable to open lockfile ...` 33 times (once per socket name it tries)
    before our error. The directory is now checked before libwayland is called.
- Missing protocol XML at build time: `[BUILD] error: cannot read protocol XML /nope/wayland.xml ... (install
  wayland/wayland-protocols or set WANA_WAYLAND_XML / WANA_WAYLAND_PROTOCOLS_DIR)`.
- Result: **PASS**

### T4: Buildroot configuration

- `make config-check`: the defconfig round-trips unchanged. It adds `BR2_PACKAGE_WAYLAND_UTILS` and
  `BR2_PACKAGE_WANA_COMPOSITOR`; `wayland`, `wayland-protocols`, `expat`, `libxml2` and `libffi` are selected
  automatically.
- Result: **PASS**

### T5: Buildroot image + `make compositor-boot-test` in CI

- The same client round trip on the real `disk.img`, with libwayland 1.24.0 and tables generated from its XML.
- Actual: *pending*

## Status

**Step 1 IN PROGRESS.** T1-T4 pass locally. T5 (CI) is pending.
