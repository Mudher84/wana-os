# Test report: Phase 10 (compositor MVP)

Phase 10 is built in steps ([decision 0001](../decisions/0001-wayland-protocol-layer.md)):
1. the protocol layer and the Wayland socket;
2. globals;
3. surfaces on screen;
4. input focus and z-order;
5. the phase report and a reproducibility re-check.

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

- Covered by the combined run in T9. The step-1-only run on `697ce89` was superseded when step 2 was pushed.
- libwayland 1.24.0's `wayland.xml` has **23** core interfaces (it adds `wl_fixes`), against 22 in the host's
  1.22. The generator needed no change:
  `[COMPOSITOR] info: protocol tables: 23 core + 5 xdg-shell interfaces (generated from XML)`.
- `ci` run 36189717968 on `697ce89`: `core_tables_match_libwayland` passes against the runner's libwayland too.
- Result: **PASS** (with T9)

## Step 2: globals

Components:
- `wana-wayland/src/server.rs`, extended into the dispatch layer:
  - one dispatcher (`wl_resource_set_dispatcher`) routes every request of every interface into the compositor's
    `Handler` trait (`bind`, `request`, `destroyed`); no per-interface C function tables;
  - request arguments are decoded into owned Rust values. Received fds become `OwnedFd`, so an unhandled fd is
    closed, not leaked;
  - events are checked before marshalling (`Ctx::post`): the opcode exists, argument kinds and count match the
    signature, NULL only where the protocol allows it, and the object's version has the event;
  - `Resource` is only an identity, and every use goes through `Ctx`, which refuses destroyed objects. Destroyed
    objects are reported to `Handler::destroyed` after the current callback, never re-entrantly;
  - destructor requests (a new `DESTRUCTORS` table from the scanner) destroy their object automatically;
  - a panic in the handler aborts the process instead of unwinding into C;
  - `request_name` gives readable log and error messages (`wl_compositor.create_surface`).
- `wana-compositor/src/globals.rs`, which advertises:

  | Global | Version | Content now | Later |
  |---|---|---|---|
  | `wl_compositor` | 4 | | surfaces and regions: step 3 |
  | `wl_shm` | 1 | formats ARGB8888, XRGB8888 | pools and buffers: step 3 |
  | `wl_output` | 4 | geometry, mode (current, preferred), scale 1, name, description, done | |
  | `wl_seat` | 7 | name `seat0`, capabilities none | pointer and keyboard: step 4 |
  | `xdg_wm_base` | 1 | pong, destroy | xdg surfaces: step 3 |

  - Versions are capped at the protocol XML's.
  - `wl_output` describes the display found through wana-drm (connector name, selected mode, physical size,
    driver). `--headless WxH@HZ` stands in on machines without one.
  - A request that belongs to a later step ends the client with `wl_display.error` (implementation) naming the
    step, instead of being ignored.

### T6: Unit tests (local)

- 57 tests (`make check`), also on Rust 1.88.
- New: signature parsing, fixed-point conversion, and request names from the tables. In `wana-compositor`, the
  versions stay within the XML's and later-step requests are refused.
- The raw-client test now covers the whole global path, byte by byte:
  - the registry advertises `wl_shm`;
  - the client binds it (untyped `new_id`: interface string plus version) and then sends `sync`;
  - it receives `format(0)`, `format(1)`, then `wl_callback.done`, then `delete_id`, in that order. The initial
    events precede the sync reply, which is what clients rely on;
  - bad events are refused before marshalling: wrong kind, wrong count, no such event;
  - on disconnect the bound object is reported destroyed, and a post to it is refused.
- Result: **PASS**

### T7: wayland-info lists the globals (local, host and QEMU)

- On the host (`--headless 1280x800@74.99`), `wayland-info` prints:
  ```
  interface: 'wl_compositor',   version:  4, name:  1
  interface: 'wl_shm',          version:  1, name:  2
          formats (fourcc):
                   1 = 'XR24'
                   0 = 'AR24'
  interface: 'wl_output',       version:  4, name:  3
          name: HEADLESS-1
          description: Wana OS headless output
          ...
          mode:
                  width: 1280 px, height: 800 px, refresh: 74.990 Hz,
                  flags: current preferred
  interface: 'wl_seat',         version:  7, name:  4
          name: seat0
          capabilities:
  interface: 'xdg_wm_base',     version:  1, name:  5
  ```
- In a QEMU boot with virtio-gpu, all 21 expectations of `make compositor-boot-test` are found, and no
  `[INIT|COMPOSITOR|DRM]` warning or error appears. The output comes from DRM:
  ```
  [DRM] info: selected Virtual-1 on CRTC 37: 1280x800@74.99 (preferred)
  [COMPOSITOR] info: globals: wl_compositor v4, wl_shm v1, wl_output v4, wl_seat v7, xdg_wm_base v1
  [COMPOSITOR] info: wl_output Virtual-1: 1280x800@74.99 Hz, 320x200 mm (Virtual-1 on virtio_gpu)
  [COMPOSITOR] info: bound wl_shm version 1 (object 4)
  [COMPOSITOR] info: bound wl_output version 4 (object 5)
  [COMPOSITOR] info: bound wl_seat version 4 (object 6)
  [COMPOSITOR] info: binds: wl_compositor 0, wl_shm 1, wl_output 1, wl_seat 1, xdg_wm_base 0
  ```
  `wayland-info` binds `wl_seat` at version 4, the highest it knows, and `wl_output` at 4.
- Result: **PASS**

### T8: A request from a later step is refused clearly (local)

- A raw client binds `wl_compositor` and calls `create_surface`:
  ```
  [COMPOSITOR] warn: wl_compositor.create_surface (object 4) not implemented yet: surfaces and regions arrive in Phase 10 step 3
  wl_display.error(object 1, code 3): wl_compositor.create_surface: surfaces and regions arrive in Phase 10 step 3
  ```
  Code 3 is `implementation`. The client is disconnected, and the compositor keeps running and shuts down
  cleanly.
- Result: **PASS**

### T9: Buildroot image + `make compositor-boot-test` (steps 1+2) in CI

- Buildroot run [36191209030](https://github.com/Mudher84/wana-os/actions/runs/36191209030), commit `9cd4982`:
  - `Build image` took 13m40s, including libwayland 1.24.0, wayland-protocols, wayland-utils, and Mesa rebuilt
    with its Wayland platform;
  - every earlier boot test still passes: kernel, initramfs, disk, graphics, GPU rendering, input.
- `make compositor-boot-test` (KVM, disk.img, virtio-gpu), 4 s:
  ```
  [COMPOSITOR] info: protocol tables: 23 core + 5 xdg-shell interfaces (generated from XML)
  [DRM] info: selected Virtual-1 on CRTC 37: 1280x800@74.99 (preferred)
  [COMPOSITOR] info: globals: wl_compositor v4, wl_shm v1, wl_output v4, wl_seat v7, xdg_wm_base v1
  [COMPOSITOR] info: wl_output Virtual-1: 1280x800@74.99 Hz, 320x200 mm (Virtual-1 on virtio_gpu)
  [COMPOSITOR] info: listening on /run/user/0/wayland-0 (WAYLAND_DISPLAY=wayland-0)
  [COMPOSITOR] info: client connected: pid 100 uid 0 gid 0
  [COMPOSITOR] info: bound wl_shm version 1 (object 4)
  [COMPOSITOR] info: bound wl_output version 4 (object 5)
  [COMPOSITOR] info: bound wl_seat version 4 (object 6)
  [COMPOSITOR] info: binds: wl_compositor 0, wl_shm 1, wl_output 1, wl_seat 1, xdg_wm_base 0
  [COMPOSITOR] info: shut down; socket removed
  [BOOT] info: absent as required: \[(INIT|COMPOSITOR|DRM)\] (warn|error)
  [BOOT] graphics test: PASS
  ```
  All 21 expectations were found, including `wayland-info`'s own listing: both shm formats, `name: Virtual-1`,
  `flags: current preferred`, `name: seat0`.
- `ci` run 36191209015 (57 unit tests, MSRV): success.
- Result: **PASS**

### T10: Console line split by a kernel message (found in the T9 run): fixed

- In the same run, the console showed:
  ```
  [INIT] info: udev: /sbin[    1.637959] udevd[77]: starting version 3.2.14
  ```
  eudev wrote a notice to `/dev/kmsg`, and the console printed it in the middle of wana-init's
  `udev: /sbin/udevd started` line. `make input-boot-test` expects that exact line. It passed in this run only
  because of timing, so it could fail at random. This is the class of problem Phase 7 recorded as a remaining
  risk, not a flake.
- Reproduced locally with eudev 3.2.14 built from the Buildroot-pinned tarball (sha256 matches `eudev.hash`) in
  a test initramfs. At the default level, udevd writes 2 lines to the console: `starting version 3.2.14` and
  `starting eudev-3.2.14`.
- Fix, in two parts:
  1. Image: `udev_log="err"` in `/etc/udev/udev.conf`, set by `post-build.sh` and idempotent. Locally this
     removes `starting version`; errors are still logged. The second line is written by `udevd.c` unconditionally
     (`fprintf(f, "<30>udevd[%u]: starting eudev-" VERSION ...)`), so configuration cannot remove it.
  2. Test harness: `tools/console_lines.py`, used by both `qemu-boot-test.sh` and `qemu-graphics-test.py`.
     - A kernel console line always starts with `[ seconds.microseconds] `. When one appears after the start of
       a line, it is moved to its own line and the interrupted text is joined with its continuation.
     - Nothing is dropped, the raw log is kept as captured, and the number of repairs is printed.
     - `--self-test` (run by `make repo-check`) covers both real splits from CI, two kernel lines inside one user
       line, untouched lines, and non-timestamps.
     - First version bug, caught locally: the firmware's screen-control codes before the first kernel line were
       taken as interrupted text and carried forward through all 424 kernel lines. A prefix of only control
       codes and whitespace is now not a split; that case is in the self-test.
     - Checked against all 24 console logs from earlier local runs: every tagged line survives, and 0 repairs
       were needed.
- The durable fix is still a separate channel for userspace logs (service/logging phase). Until then the harness
  matches whole lines even when the kernel interrupts them.
- CI: buildroot run [36195465287](https://github.com/Mudher84/wana-os/actions/runs/36195465287) and `ci` run
  36195465285 on `66af093` are both green:
  - all boot tests pass, including input and compositor;
  - in every boot shown in the log tail (graphics, GPU rendering, input, compositor),
    `[INIT] info: udev: /sbin/udevd started (pid N)` arrived whole, and eudev's `starting version` notice no
    longer appears in any of those lines;
  - the harness reported no repairs in these boots.
- Result: **PASS**

### T11: Reproducibility with the Wayland stack

- Reproducibility run [36198390819](https://github.com/Mudher84/wana-os/actions/runs/36198390819) on `develop` @
  `7624eaa`. This is the first run with libwayland 1.24.0, wayland-protocols, wayland-utils, Mesa with its Wayland
  platform, and `wana-compositor` (tables generated by `build.rs` from staging XML) in the image.
- Build A (ccache) took 68 min. Build B (`CCACHE_DISABLE=1`) took 69 min.
- `compare A vs B`: `[BUILD] reproducibility: PASS (12/12 artifacts identical)`, from bzImage to `disk.img`.
- The generated protocol tables are deterministic: the XML is read in file order, no paths or timestamps are
  embedded, and the build-time XML path does not end up in the binary.
- Buildroot run 36198390808 and `ci` run 36198390857 on the same commit: success.
- Result: **PASS**

## Step 3: client windows on screen

Components:
- `wana-compositor/src/shm.rs`: wl_shm pools and buffers.
  - The client file is mapped read-only and shared. `fstat` must show the file is at least as large as the pool
    (`invalid_fd` otherwise). Pools only grow.
  - Buffer layouts are checked with 64-bit arithmetic: format ARGB/XRGB8888, `stride >= width*4`, and the last row
    inside the pool.
  - SIGBUS guard: a client can shrink its file after creating the pool. While a pool is being read, a SIGBUS
    handler maps zero pages over exactly that pool (`MAP_FIXED`, anonymous); the copy continues and the pool is
    then marked poisoned. The client receives `wl_shm.error(invalid_fd)` and the compositor keeps running.
    A SIGBUS anywhere else is left fatal, so real bugs still crash visibly. libwayland's own shm code uses the
    same technique.
- `surface.rs`: pure state machines, unit-tested without libwayland.
  - Double-buffered surface state: attach, frame, scale.
  - The xdg handshake:
    - initial commit without a buffer, then `xdg_toplevel.configure` + `xdg_surface.configure(serial)`;
    - ack, then attach and commit: the window is mapped;
    - a NULL buffer unmaps it.
  - Ack bookkeeping: acking a serial acknowledges the older ones; unknown or repeated serials are
    `invalid_serial`.
  - Placement: centered, then cascaded by 32 px, clamped to the screen.
- `globals.rs`: the handler for wl_compositor, wl_region, wl_shm, wl_shm_pool, wl_buffer, wl_surface
  (attach, damage, damage_buffer, frame, commit, transform normal, scale 1), xdg_wm_base (create_positioner,
  get_xdg_surface), xdg_surface (get_toplevel, ack_configure) and xdg_toplevel (title, app_id).
  - Protocol errors with the XML's codes: `xdg_wm_base.role` / `invalid_surface_state`,
    `xdg_surface.not_constructed` / `already_constructed` / `unconfigured_buffer` / `invalid_serial`,
    `wl_surface.invalid_scale`, `wl_shm.*`.
  - Popups, buffer transforms and scale factors other than 1 end the client with an implementation error that
    names what is missing.
- Buffer model, copy at commit: the pixels are copied under the SIGBUS guard (rows packed), uploaded to a GLES
  texture, and `wl_buffer.release` is sent immediately. Client memory is never read after the commit, so a client
  cannot change or truncate a buffer while the compositor draws from it, and it can reuse the buffer right away.
  The cost is one copy per commit. zero-copy buffers (linux-dmabuf) come later.
- `wana-render/src/compose.rs`:
  - textures from shm pixels, uploaded as RGBA bytes and swizzled with `.bgra` in the shader, so no BGRA texture
    extension is needed;
  - XRGB alpha forced to 1, premultiplied-alpha blending for ARGB;
  - quads in output pixel coordinates with a top-left origin.
- `render.rs`: the screen.
  - GBM surface + EGL + composer. The first frame uses SETCRTC, later frames a page flip, with at most one in
    flight.
  - The buffer that was on screen goes back to GBM only after the flip that replaces it completes, so the
    scanned-out buffer is never drawn into (no tearing).
- Event loop: one `poll()` on the Wayland event loop fd and the DRM fd, single-threaded.
  - A redraw happens when something changed and no flip is pending; commits during a flip wait for it, so there
    is at most one frame per vblank.
  - Frame callbacks fire when the frame is actually on screen, carrying the flip timestamp (ms), not at swap time.
  - Headless mode uses a 16 ms clock instead.
- `crates/wana-wl-test`: test client on libwayland-client (FFI, generated tables; events through one dispatcher
  into a queue).
  - Default: a 480x320 XRGB8888 memfd buffer with the Wana palette (accent, 8 px white border). The client waits
    for the frame callback, then holds.
  - `--attach-before-configure` and `--truncate-pool` are deliberate protocol violations that must end in the
    expected error.
- Buildroot:
  - `wana-compositor` now depends on Mesa (libegl, libgles, libgbm) and installs `wana-wl-test`.
  - `make window-boot-test` (screenshot + pixels) and `make wayland-host-test` (three scenarios, headless) run in
    CI.

### T12: Unit tests (local)

- 70 tests, on the pinned toolchain and on Rust 1.88.
- New:
  - shm: layout validation including overflow-sized values, packing rows out of a strided pool, a pool larger than
    its file, grow-only resize;
  - the SIGBUS test: a real memfd truncated after mapping; the read fails with `invalid_fd` "SIGBUS", the test
    process survives, and the pool stays poisoned;
  - surface: the handshake table, ack bookkeeping, a buffer without a role object, placement;
  - compose: color split, shader swizzle and origin;
  - test client: the pattern bytes (B, G, R order).
- One test was wrong at first: it expected an "overflow" rejection for a layout whose end offset (about 2^62) fits
  in `u64` and was smaller than the pool size given. Real pools are at most `i32::MAX` bytes, and against that
  limit the layout is refused. The test now uses that limit; the code was already right.
- Result: **PASS**

### T13: Three protocol scenarios against the real libwayland (local, headless)

- `make wayland-host-test`, using the host's libwayland-server 1.22 and libwayland-client:
  ```
  [COMPOSITOR] check: window scenario: PASS
  [COMPOSITOR] check: buffer before configure -> xdg_surface.unconfigured_buffer: PASS
  [COMPOSITOR] check: truncated pool (SIGBUS) -> wl_shm.invalid_fd, compositor survives: PASS
  ```
- Window scenario log:
  ```
  [COMPOSITOR] info: client: configure received (serial 1); acking
  [COMPOSITOR] info: window mapped: "wana-wl-test" (org.wana.test) 480x320 at 400,240 (surface 8)
  [COMPOSITOR] info: client: frame presented (callback done at 19 ms); buffer released
  [COMPOSITOR] info: window closed by client: "wana-wl-test"
  ```
- Protocol errors as the client saw them:
  ```
  xdg_surface@9: error 3: buffer attached before the first configure was acknowledged
  [COMPOSITOR] warn: shm error 2: pool file was truncated while in use (SIGBUS)
  wl_buffer@7: error 2: pool file was truncated while in use (SIGBUS)
  [COMPOSITOR] info: shut down; socket removed
  ```
- Result: **PASS**

### T14: A client window on the screen (local, QEMU TCG, virtio-gpu, host Mesa llvmpipe)

- `wana-compositor --run wana-wl-test --hold 5` in a QEMU boot, with the expectations and pixels of
  `make window-boot-test`: 9 of 9 found, no `[INIT|COMPOSITOR|DRM|RENDER]` warning or error.
  ```
  [RENDER] info: compositor renderer: GL_RENDERER=llvmpipe (LLVM 20.1.2, 128 bits), 1280x800
  [DRM] info: modeset done: Virtual-1 1280x800@74.99 (preferred) on CRTC 37; compositor frame on screen
  [COMPOSITOR] info: frame 1: 0 window(s) on screen
  [COMPOSITOR] info: window mapped: "wana-wl-test" (org.wana.test) 480x320 at 400,240 (surface 8)
  [COMPOSITOR] info: frame 2: 1 window(s) on screen
  [COMPOSITOR] info: client: frame presented (callback done at 13984 ms); buffer released
  [COMPOSITOR] info: frame 3: 0 window(s) on screen
  ```
  Frame 3 is the redraw after the client closed its window.
- Screenshot 1280x800, all exact:
  - (640,400) `#4f8cff`: the window interior;
  - (402,400) and (640,243) `#ffffff`: the left and top borders;
  - (128,400) and (0,0) `#16213e`: the compositor background.
  The PNG shows the window centered, not flipped, with an even border.
- `make compositor-boot-test` (step 2) still passes with the compositor now drawing.
- Result: **PASS**

### T15: Buildroot image + `make window-boot-test` and `make wayland-host-test` in CI

- Buildroot run [36235233543](https://github.com/Mudher84/wana-os/actions/runs/36235233543), commit `3fedbe6`:
  every step passed, including the new "Client window" step.
  ```
  [RENDER] info: compositor renderer: GL_RENDERER=softpipe, 1280x800
  [DRM] info: modeset done: Virtual-1 1280x800@74.99 (preferred) on CRTC 37; compositor frame on screen
  [COMPOSITOR] info: window mapped: "wana-wl-test" (org.wana.test) 480x320 at 400,240 (surface 8)
  [COMPOSITOR] info: frame 2: 1 window(s) on screen
  [COMPOSITOR] info: client: frame presented (callback done at 1666 ms); buffer released
  [BOOT] info: pixel (640,400) = #4f8cff expected #4f8cff
  [BOOT] info: pixel (402,400) = #ffffff expected #ffffff
  [BOOT] info: pixel (640,243) = #ffffff expected #ffffff
  [BOOT] info: pixel (128,400) = #16213e expected #16213e
  [BOOT] info: pixel (0,0) = #16213e expected #16213e
  [BOOT] graphics test: PASS (log: out/logs/window-boot.log)
  ```
  9 of 9 expectations were found, and no `[INIT|COMPOSITOR|DRM|RENDER]` warning or error appeared. The
  Buildroot Mesa has no llvmpipe (no LLVM in the image), so rendering used softpipe; the frame was still on screen
  1.7 s after the compositor started (with KVM).
- `ci` run 36235233534 on `3fedbe6` (unit tests, MSRV, `make wayland-host-test` against the runner's
  libwayland): success.
- Result: **PASS**

## Step 4: input and focus

Components:
- `wana-input`:
  - `Keyboard::keymap_string` (xkb text v1), `Keyboard::from_string` (the client side);
  - `modifiers()`: the depressed, latched and locked masks plus the effective layout, as wl_keyboard.modifiers
    carries them;
  - `set_modifiers` (the client follows the compositor's state);
  - `lookup` (what a key means without changing the state);
  - `key()` now reports whether the key changed the xkb state.
- `wana-compositor/src/seat.rs`: pure seat state, unit-tested without libwayland.
  - Capabilities come from the devices present, recomputed on hotplug. The seat also remembers which capabilities
    it has ever had, because get_pointer/get_keyboard are protocol violations only for a capability the seat
    never had.
  - Cursor position: relative motion is accumulated, absolute devices are scaled to the output, and the result is
    clamped to the output.
  - Hit test: the topmost window under the cursor, using surface-local coordinates.
  - Implicit grab: while a button is held, pointer focus stays on the surface where the press landed.
  - Held keys: repeated presses and releases of keys that were not down are dropped; the held keys fill
    wl_keyboard.enter's array.
  - Destroyed focus or cursor surfaces are forgotten.
  - The default arrow cursor is generated in code: premultiplied, white with a black outline, and only fully opaque
    or fully transparent pixels.
- `input.rs`: libinput events become protocol events. Each event goes only to the objects of the client that owns
  the focused surface (new `Ctx::client`).
  - wl_pointer v7:
    - events: enter, leave, motion, button, and axis for wheels (axis_source, axis_discrete, and axis with 15 per
      detent); each group of events ends with a frame;
    - set_cursor: honored only from the focused client with the serial of its latest enter. NULL hides the
      cursor; a surface gets the cursor role, or the request fails with `wl_pointer.role` if the surface already
      has another role.
  - wl_keyboard v7:
    - `keymap` (xkb v1): one sealed memfd shared by every client. It is sealed with `F_SEAL_WRITE`,
      `F_SEAL_SHRINK`, `F_SEAL_GROW` and `F_SEAL_SEAL`, so no client can change what the others read;
    - `enter` with the held keys, followed by `modifiers`;
    - `leave`; `key`;
    - `modifiers` after every key that changes the xkb state;
    - `repeat_info` (25 per second after 600 ms; clients do the repeating).
  - Touch: `get_touch` fails with `missing_capability`.
- Focus rules:
  - Pointer focus follows the cursor, except during a grab. A window mapped, unmapped or destroyed under the
    cursor moves pointer focus without any pointer motion: `sync_focus` runs after every dispatch.
  - A new window gets keyboard focus. A click raises the window under the cursor and gives it keyboard focus.
  - When the focused window goes away, keyboard focus moves to the topmost remaining window.
- Cursor: drawn in software, on top of the scene, after a pointing device has been used for the first time. The
  window-only screenshots are therefore unaffected. A hardware cursor plane comes later.
- Timestamps: input events use CLOCK_MONOTONIC in milliseconds, the same clock as the DRM flip times used for frame
  callbacks.
- Latency fix found while doing this step: events the compositor posts outside a dispatch (frame callbacks, and now
  input) were only flushed at the next dispatch, which could be up to one 100 ms poll timeout later.
  `Display::with_handler` now flushes the clients when it returns.
- Event loop: the libinput fd is part of the same `poll()`. Input is opened only with a real display; headless
  runs are protocol tests without devices.
- `wana-wl-test` was split into modules: `window.rs` creates and maps a window, `input.rs` handles the seat and
  focus, and `main.rs` holds the scenarios.
- `wana-wl-test --input TEXT` (one window, "main"):
  1. binds wl_seat and waits until the seat has both a pointer and a keyboard;
  2. maps the keymap fd with `MAP_PRIVATE` and compiles it;
  3. maps its window and waits for keyboard focus, then logs `client: ready for input`;
  4. succeeds once the pointer has entered the window, a left click has arrived, and TEXT has been typed. The text
     is decoded with the compositor's keymap and modifier state.
  
  While doing this, a client bug was fixed: waiting for the configure event discarded every other event, including
  the keymap and the keyboard enter. Every event now also goes to the input handler, and events already queued are
  handled before the client blocks.
- `wana-wl-test --zorder TEXT` (two windows):
  - "back" is 1400x900, larger than the screen, so it is placed at 0,0 and covers the screen (amber);
  - "front" is 480x320 in the accent color, mapped second, so it is on top and gets keyboard focus;
  - the client then expects a click on "back" (visible only outside "front", e.g. in the top-left corner), keyboard
    focus moving to "back", and TEXT typed into it;
  - on every pointer enter, the client sets a 64x64 magenta surface (no role, buffer committed) as its cursor, with
    hotspot 0,0.
- Buildroot: `wana-compositor` depends on udev, libinput, libxkbcommon and xkeyboard-config.
- `make seat-boot-test` (in CI) runs the z-order scenario:
  - QEMU drives the pointer into the corner with large relative moves. The cursor clamps at the screen edge, so the
    position does not depend on pointer acceleration.
  - QEMU then clicks and types Shift+W a n a.
  - The screenshot pixels must show "back" above "front".
- T8 no longer applies to wl_seat: its requests are implemented now.

### T16: Unit tests (local)

- 81 tests (70 before this step).
- New:
  - keymap text round trip (fr layout, compiled back on the client side);
  - the client following the modifier state (`set_modifiers`: Shift turns `q` into `Q`);
  - serialized modifiers (Shift = bit 0, `mods_changed` only for modifier keys);
  - hit test (top window wins, right and bottom edges are outside);
  - cursor clamping and absolute positions;
  - implicit grab;
  - held keys and the enter array;
  - capabilities on hotplug, with the history kept;
  - forgetting destroyed focus and cursor surfaces;
  - the arrow's pixels;
  - the keymap memfd: contents plus NUL; write, shrink and grow refused by the seals;
  - the monotonic clock.
- Result: **PASS**

### T17: Keys and mouse reach the focused window (local, QEMU TCG, virtio input + PS/2, host Mesa)

- QEMU sends `sendkey shift-w`, `a`, `n`, `a`, then `mouse_move 40 30` and one left click, when the client logs
  that it is ready.
  ```
  [INPUT] info: keymap English (US) for clients: 64756 bytes, sealed memfd
  [INPUT] info: seat0 capabilities: keyboard
  [INPUT] info: seat0 capabilities: pointer, keyboard
  [COMPOSITOR] info: client: seat capabilities 0x3 (pointer + keyboard)
  [COMPOSITOR] info: client: keymap received: 64756 bytes, layout English (US)
  [COMPOSITOR] info: window mapped: "wana-wl-test" (org.wana.test) 480x320 at 400,240 (surface 8)
  [COMPOSITOR] info: keyboard focus: "wana-wl-test" (org.wana.test)
  [COMPOSITOR] info: client: keyboard focus on the window (0 key(s) held)
  [COMPOSITOR] info: client: ready for input (keyboard focus, keymap compiled)
  [COMPOSITOR] info: client: key 42 pressed: Shift_L text ""
  [COMPOSITOR] info: client: key 17 pressed: W text "W"
  [COMPOSITOR] info: client: key 30 pressed: a text "a"
  [COMPOSITOR] info: client: key 49 pressed: n text "n"
  [COMPOSITOR] info: client: key 30 pressed: a text "a"
  [COMPOSITOR] info: client: pointer entered the window at 262.0,176.5
  [COMPOSITOR] info: client: left click at 262.0,176.5
  [COMPOSITOR] info: client: input received: typed "Wana", pointer entered, 0 motion event(s), 1 left click(s)
  ```
  - The capital W shows the modifiers path works: the client decodes keys with its own xkb state, which it updates
    only from the compositor's wl_keyboard.modifiers.
  - The pointer moved relatively on the PS/2 mouse, from the output center (640,400) by the accelerated
    (22,16.5), and entered the window at (262,176.5), which is 662-400 and 416.5-240.
  - The enter carried the position, so no separate motion was needed.
- All 15 expectations of the first, single-window version of `make seat-boot-test` matched this log. T18 replaced that
  version with the z-order scenario, and the client's log lines now name the window. None of its reject patterns matched: no
  `[INIT|COMPOSITOR|DRM|RENDER|INPUT]` warning or error.
- Screenshot: the arrow is drawn with its tip at (661,416), black outline and white fill, over the window. The
  pointer x was 661.99; the tip pixel is the one that contains that position.
- Result: **PASS**

### T18: Click raises and focuses, typing follows the focus (local, QEMU TCG, the exact `make seat-boot-test` arguments)

- `wana-wl-test --zorder Wana`:
  ```
  [COMPOSITOR] info: window mapped: "wana-wl-test back" (org.wana.test) 1400x900 at 0,0 (surface 11)
  [COMPOSITOR] info: keyboard focus: "wana-wl-test back" (org.wana.test)
  [COMPOSITOR] info: client: keyboard focus on window "back" (0 key(s) held)
  [COMPOSITOR] info: window mapped: "wana-wl-test" (org.wana.test) 480x320 at 432,272 (surface 17)
  [COMPOSITOR] info: keyboard focus: "wana-wl-test" (org.wana.test)
  [COMPOSITOR] info: client: keyboard focus left window "back"
  [COMPOSITOR] info: client: keyboard focus on window "front" (0 key(s) held)
  [COMPOSITOR] info: client: ready for input (keyboard focus on window "front", keymap compiled)
  [COMPOSITOR] info: client: pointer entered window "front" at 95.8,15.8
  [COMPOSITOR] info: client: pointer left window "front"
  [COMPOSITOR] info: client: pointer entered window "back" at 295.0,55.0
  [COMPOSITOR] info: window raised: "wana-wl-test back" (org.wana.test)
  [COMPOSITOR] info: client: keyboard focus left window "front"
  [COMPOSITOR] info: client: keyboard focus on window "back" (0 key(s) held)
  [COMPOSITOR] info: client: left click on window "back" at 17.6,17.6
  [COMPOSITOR] info: client: input received: typed "Wana" on window "back", left click on window "back", 11 motion event(s)
  ```
  (The motion and click coordinates are from the first run of this scenario. The run with the exact CI arguments
  matched the same expectations.)
- Each rule shows in the log:
  - a newly mapped window takes keyboard focus;
  - the pointer's focus follows the cursor across windows, with a leave before each enter;
  - the button press raises the window under the cursor and moves keyboard focus to it (leave on the old window,
    enter on the new) before the button event itself is delivered;
  - typing goes to the new focus.
- Screenshot: (640,400), (384,400) and (1152,720) are all `#e0a030`. "back" is now drawn above "front", which
  covered the screen's center before the click.
- Client cursor (added to the scenario after the first run):
  ```
  [COMPOSITOR] info: cursor: client surface 64x64, hotspot 0,0 (from "wana-wl-test back" (org.wana.test))
  [BOOT] info: pixel (60,60) = #ff00ff expected #ff00ff
  ```
  - The pointer ends within 60 px of the corner, whatever the acceleration: it is clamped into the corner, then
    moved a little. So pixel (60,60) is always under the 64x64 cursor. It is magenta: the client's surface is
    drawn in place of the arrow.
  - The set_cursor answering the enter on "front" was ignored as stale. By the time it arrived, the pointer had
    already moved to "back", so its serial no longer matched the latest enter. That is the rule the compositor
    enforces, observed with real timing.
- All 19 expectations and the 4 pixel checks of `make seat-boot-test` passed, and no `[INIT|COMPOSITOR|DRM|RENDER|INPUT]`
  warning or error appeared.
- Result: **PASS**

### T19: Regressions (local)

- `make wayland-host-test`: 3 of 3 scenarios pass (headless runs open no input).
- The window test (`wana-wl-test --hold 5`, no input sent) with the new binaries: all 5 pixels exact. The cursor
  stays hidden until a pointing device is used.
- Result: **PASS**

### T20: Buildroot image + `make seat-boot-test` in CI

- Actual: *pending*

## Step 5: phase close-out

### Exit criteria (decision 0001, Phase 10 plan)

| Step | Exit test | Evidence |
|---|---|---|
| 1 | The socket comes up, and the generated tables match the XML and the libwayland they run with | T1-T5 |
| 2 | `wayland-info` in the image lists every global | T7, T9 |
| 3 | The test client's color reaches the screen (screenshot pixels) | T14, T15 |
| 4 | A QEMU click and typing reach the focused client; a click raises and focuses | T17, T18, T20 |
| 5 | Reproducibility with the whole compositor stack | T21 |

### Known limits (deliberate; later phases or decisions)

- Buffers are copied at commit time: one copy and one texture upload per commit. linux-dmabuf (zero copy) comes
  later.
- The whole frame is redrawn on every change; damage tracking is not used yet.
- Surfaces:
  - popups (`xdg_popup`), subsurfaces, buffer transforms and scales other than 1 are refused with a clear
    implementation error;
  - opaque and input regions are ignored, so hit tests use the whole surface.
- Window management: new windows are placed centered, then cascaded. There is no move, resize, maximize or
  decorations yet (Phase 12).
- Input:
  - no touch or tablet support (`get_touch` fails with `missing_capability`);
  - one keymap for the whole seat, chosen with `--layout` (no layout switching yet);
  - clients do the key repeat (from `repeat_info`);
  - the cursor is drawn in software (no hardware cursor plane yet);
- One output only. No output hotplug, and the output scale is 1.
- Frame callbacks also fire for surfaces that are not visible.
- Single-threaded: protocol, input and rendering share one `poll()` loop. With softpipe (CI) a frame costs a lot of
  CPU; the image has no GPU acceleration under QEMU.

## Status

- Step 1: **PASS** (T1-T5).
- Step 2: **PASS** (T6-T9).
- T10: console-split robustness fix: **PASS** (local and CI).
- T11: reproducibility with the Wayland stack: **PASS** (12/12).
- Step 3: **PASS** (T12-T15).
- Step 4: T16-T19 pass locally; T20 (CI) is pending.
