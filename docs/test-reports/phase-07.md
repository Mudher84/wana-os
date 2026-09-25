# Test report: Phase 7 (Native DRM/KMS)

Components:
- `crates/wana-drm`: DRM/KMS backend in Rust, **no libdrm and no crates.io dependencies**.
  The kernel uAPI is used directly (`src/sys.rs`: ioctl numbers and `repr(C)` structs).
  - `discover.rs` enumerates `/sys/class/drm/cardN` (never assumes `card0`) with each card's device driver and connectors.
  - `card.rs` wraps the ioctls: VERSION, GET_CAP, GETRESOURCES, GETCONNECTOR, GETENCODER, CREATE/MAP/DESTROY_DUMB,
    ADDFB/RMFB, SETCRTC, PAGE_FLIP, and parsing of flip-complete events.
  - `mode.rs`: the preferred resolution at its highest refresh rate. Refresh is computed from the pixel clock, so
    59.94 Hz and 60 Hz are told apart.
- `wana-kms` (`/usr/bin/wana-kms`): discover → open → connectors/modes → CRTC → 2 dumb buffers with a test
  pattern → modeset → N page flips with kernel flip timestamps → hold. Every step logs a `[DRM]` line.
- `wana-init`: new `wana.run=/abs/path[,args]` option runs one program after `ready` (bring-up/tests).
- `tools/qemu-graphics-test.py`: boots with a real display device (`virtio-gpu` or `std`/bochs), takes a QEMU
  monitor `screendump` when a log line appears, checks pixel colors, and writes a PNG.

## T1: uAPI layout matches the kernel headers (local)

- Expected values: a C program compiled against `<drm/drm.h>` and `<drm/drm_mode.h>` printed `sizeof` for 15 structs
  and the values of 14 `DRM_IOCTL_*` numbers (e.g. `drm_mode_get_connector` 80, `drm_mode_crtc` 104,
  `DRM_IOCTL_MODE_GETCONNECTOR` 0xc05064a7)
- Tests `struct_sizes_match_kernel_headers` and `ioctl_numbers_match_kernel_headers` assert all of them
- Result: **PASS**

## T2: Unit tests, lint, MSRV (local)

- `make check`: wana-drm 13 tests (layout, ioctl numbers, sysfs discovery on a fake tree, mode selection incl.
  120 Hz preference and 59.94 vs 60 Hz, connector names, flip-event parsing incl. truncated input), wana-init 10,
  wana-log 6; clippy `-D warnings` clean. `make msrv` (Rust 1.88): pass
- Result: **PASS**

## T3: Native frame on screen, virtio-gpu (local, initramfs)

- Kernel: Phase 3 bzImage. Initramfs: static wana-init + wana-kms. QEMU TCG + OVMF, `-device virtio-gpu-pci`.
- `wana.run=/usr/bin/wana-kms,--hold,4 wana.test=poweroff`
- Log:
  ```
  [DRM] info: found card0 (/dev/dri/card0) device-driver=virtio-pci connectors=[Virtual-1]
  [DRM] info: card0: opened, driver virtio_gpu 0.1.0
  [DRM] info: card0: 1 connector(s), 1 encoder(s), 1 CRTC(s), max 8192x8192
  [DRM] info: connector Virtual-1 (id 38): Connected, 26 mode(s), 320x200 mm
  [DRM] info: selected Virtual-1 on CRTC 37: 1280x800@74.99 (preferred)
  [DRM] info: framebuffer 0: fb 43, 1280x800 XRGB8888, pitch 5120 bytes
  [DRM] info: modeset done: Virtual-1 1280x800@74.99 (preferred) on CRTC 37; frame on screen
  [DRM] info: page flip: 120 flips completed, interval mean 1.22 ms (822.9 Hz), ...; timestamps monotonic
  ```
- Screenshot 1280x800, pixels: center `#4f8cff` (accent), (128,400) `#16213e` (background), (0,0) `#ffffff`
  (border). All exact. The border and centered square also check that the pitch is honored.
- Result: **PASS**

## T4: Second driver, QEMU std VGA → bochs-drm (local)

- Same test with `--gpu std`: `device-driver=bochs-drm`, driver `bochs-drm 1.0.0`, modeset done, 3/3 pixels exact
- Result: **PASS**. Discovery and modesetting do not depend on one driver.

## T5: Full disk path: OVMF → GRUB → kernel → ext4 → wana-init → wana-kms (local)

- Command: `make graphics-boot-test` against a locally assembled `disk.img`
- The firmware framebuffer (UEFI GOP, `simpledrm`) is handed over to `virtio_gpu` by the kernel
  (`[drm] Initialized virtio_gpu 0.1.0 ... on minor 0`). wana-kms finds exactly one card, `card0` = virtio_gpu.
- All 6 expected log lines found, 3/3 pixels exact, clean power off
- Result: **PASS**

## T6: Graphics test detects failures (local)

- Wrong expected color (`0.5,0.5=ff0000`): `pixel (640,400) = #4f8cff expected #ff0000`, FAIL, exit 1
- Trigger line never appears (no `wana.run`): `no screenshot was taken`, FAIL, exit 1
- Result: **PASS**

## Known limit: vsync pacing is not verified yet

The page-flip loop works (120/120 flip-complete events, monotonic timestamps), but the measured intervals
are not the display refresh:
- virtio-gpu: mean 1.22 ms (~820 Hz);
- bochs-drm: 6.66 ms (~150 Hz), with the mode at 74.99 Hz.

Neither QEMU device has a real vblank: flips complete as soon as the host has processed them. The code waits
for the kernel's flip-complete event before each flip, so it paces correctly on hardware with real vblank.
**Proving** that needs real hardware (Phase 28) or a kernel driver with a vblank timer (e.g. vkms, not built into the
production kernel). It is not claimed here.

## T7: Buildroot image + graphics test in CI

- Run [36142568529](https://github.com/Mudher84/wana-os/actions/runs/36142568529), commit `0e69d35`, job `toolchain, image, UEFI boot` (20m35s)
- `make image` built the new `wana-drm` package with Buildroot's toolchain and Rust 1.88. The earlier tests still pass:
  kernel config, kernel alone, kernel + initramfs, disk through GRUB.
- `make graphics-boot-test` on the Buildroot `disk.img` (OVMF → GRUB → kernel → ext4 → wana-init → wana-kms,
  virtio-gpu, KVM):
  ```
  [BOOT] info: found: \[DRM\] info: found card[0-9]+
  [BOOT] info: found: \[DRM\] info: selected .* on CRTC [0-9]+
  [BOOT] info: found: \[DRM\] info: modeset done
  [BOOT] info: found: \[DRM\] info: page flip: [0-9]+ flips completed
  [BOOT] info: found: \[INIT\] info: /usr/bin/wana-kms exited successfully
  [BOOT] info: found: reboot: Power down
  [BOOT] info: screenshot 1280x800 -> out/test/wana-kms.png
  [BOOT] info: pixel (640,400) = #4f8cff expected #4f8cff
  [BOOT] info: pixel (128,400) = #16213e expected #16213e
  [BOOT] info: pixel (0,0) = #ffffff expected #ffffff
  [BOOT] graphics test: PASS
  ```
- Artifact `wana-image-<sha>` includes the screenshot PNG
- Result: **PASS**

## Phase 7 status

**PASS.** T1-T7 all pass. Wana OS drives the display natively: its own Rust
DRM/KMS code discovers the card, sets the mode and shows a frame. The frame is verified
pixel by pixel on the Buildroot-built image, booted the way hardware boots.
Open item carried forward: real vsync pacing (see "Known limit").

## Post-PASS finding: console line split by a kernel message (PR #5, run 36157632246): **FAIL**, fixed

- The `graphics-boot-test` step failed on the PR #5 head (`2d670c5`). The product worked: wana-kms did the modeset,
  completed 120 flips, the 3/3 pixels were exact, and it exited 0. But the expectation
  `\[INIT\] info: /usr/bin/wana-kms exited successfully` did not match, because a kernel message was printed into the
  middle of that console line:
  ```
  [INIT] info: /us[    6.502726] wana-kms (78) used greatest stack depth: 12872 bytes left
  r/bin/wana-kms exited successfully
  ```
- Root cause: `x86_64_defconfig` sets `CONFIG_DEBUG_STACK_USAGE=y`. `kernel/exit.c` then prints `<task> used greatest
  stack depth` with `pr_info` whenever an exiting task sets a new stack-depth record. That happens at random moments
  (earlier runs happened not to collide), and the serial console interleaves it with userspace output.
- Fix: `# CONFIG_DEBUG_STACK_USAGE is not set` in `linux.fragment`. It is a debugging aid with runtime cost, not
  wanted in Wana images. The kernel config check now verifies 50 options.
- Remaining risk, recorded rather than hidden: any asynchronous kernel message can still split a userspace console
  line. The durable fix is to give userspace logs their own channel (for example a second serial port or `/dev/kmsg`
  records). That is planned with the service/logging work, not in this PR.
