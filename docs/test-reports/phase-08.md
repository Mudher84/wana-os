# Test report: Phase 8 (GBM / EGL / OpenGL ES)

Components:
- `crates/wana-render`: small FFI declarations for libgbm, libEGL 1.5 and libGLESv2 (`src/ffi.rs`), plus safe wrappers:
  - `gbm.rs`: device, scanout+rendering surface, front-buffer lock and release;
  - `egl.rs`: platform display on GBM (`EGL_PLATFORM_GBM_KHR`), an ES2-compatible config whose native visual is XRGB8888,
    context, window surface;
  - `gl.rs`: strings, shader compile/link with info logs, error checks;
  - `scene.rs`: the test scene as one GLSL ES 1.00 fragment shader (rounded square via a signed distance field, border,
    animated progress bar), with the same palette as the CPU pattern of Phase 7.
  No crates.io dependencies.
- `wana-gl` (`/usr/bin/wana-gl`): `wana_drm::output::find()`, then GBM, EGL, GLES and the scene, then eglSwapBuffers,
  lock front bo, ADDFB from the bo handle/stride, SETCRTC (first frame) and PAGE_FLIP with flip events (next frames),
  releasing the previous bo only after its replacement is on screen. Logs `[DRM] [GBM] [EGL] [RENDER]`.
- `wana-drm` refactor: output selection moved from the `wana-kms` binary into `wana_drm::output` (shared by both
  tools), plus `Card::add_fb_handle` and `Card::raw_fd`.
- Buildroot: `mesa3d` 26.0.1 with the gallium `softpipe` and `virgl` drivers, EGL, GLES and GBM. The `wana-render` package
  depends on `libegl libgles libgbm`.

## T1: Unit tests, lint, MSRV (local)

- `make check`: 43 tests (wana-render 4: XRGB8888 fourcc = 0x34325258, gbm_bo_handle size, GLSL palette round-trip to
  8-bit, shader palette embedding; wana-drm 13; wana-init 10; wana-log 6); clippy `-D warnings` clean; builds on Rust 1.88
- Result: **PASS**

## T2: wana-kms unchanged after the refactor (local)

- Phase 7 graphics test re-run with the refactored binary: `selected Virtual-1`, `modeset done`, 3/3 pixels exact
- Result: **PASS**

## T3: GPU-rendered frame on screen (local, initramfs, host Mesa 25.2.8)

- Setup: wana-gl built against the host's Mesa; initramfs with glibc, libgbm, libEGL (glvnd + Mesa vendor),
  libGLESv2, `dri_gbm.so`, the gallium DRI drivers. QEMU TCG + OVMF + virtio-gpu (no virgl 3D on this host)
- Log:
  ```
  [GBM] info: device created, backend drm
  [GBM] info: surface 1280x800 XRGB8888 (scanout | rendering)
  [EGL] info: EGL 1.5 vendor=Mesa Project version="1.5" apis="OpenGL OpenGL_ES "
  [EGL] info: OpenGL ES context current on GBM window surface
  [RENDER] info: GL_VENDOR=Mesa GL_RENDERER=llvmpipe (LLVM 20.1.2, 128 bits)
  [RENDER] info: GL_VERSION=OpenGL ES 3.2 Mesa 25.2.8-0ubuntu0.24.04.2 GLSL=OpenGL ES GLSL ES 3.20
  [RENDER] info: shaders compiled and linked
  [DRM] info: modeset done: Virtual-1 1280x800@74.99 (preferred) on CRTC 37; first GPU frame on screen (fb 44, stride 5120)
  [RENDER] info: 20 frames rendered, GPU frame time mean 325.96 ms, max 2181.55 ms (3.1 fps possible)
  [DRM] info: page flip: 19 flips completed, interval mean 230.76 ms
  ```
- Screenshot 1280x800: center `#4f8cff`, (128,400) `#16213e`, (0,0) `#ffffff`, all exact. The rounded corners and
  the progress bar are visible in `out/test/wana-gl-local.png`.
- Result: **PASS**

## T4: Full disk path with `make gl-boot-test` (local)

- OVMF → GRUB → kernel → ext4 root (host-Mesa test rootfs) → wana-init → wana-gl, virtio-gpu
- All 9 expected lines found ([GBM] surface, [EGL] version and context, [RENDER] vendor/shaders/frames,
  [DRM] first GPU frame, wana-gl exit 0, power off). 3/3 pixels exact.
- Result: **PASS**

## T5: A broken layer is reported at that layer (local)

- Same image with Mesa's GBM backend (`gbm/dri_gbm.so`) removed
- Log: `MESA-LOADER: failed to open dri: .../gbm/dri_gbm.so ...`, then `[GBM] error: gbm_create_device failed`,
  then `[INIT] error: /usr/bin/wana-gl failed: exit status: 1`. The test fails (exit 1).
- Result: **PASS**. The first failing layer (GBM) is named, and nothing after it runs.

## Notes on performance (not a Phase 8 exit criterion)

The local frame times (326-439 ms) combine QEMU **TCG** (full CPU emulation, no KVM in the development sandbox)
with **software** rasterization. They say nothing about Wana OS performance. The Buildroot image uses
Mesa `softpipe` (no LLVM, to keep build times down); `llvmpipe` and real GPU drivers are Phase 27/28 decisions.
`virgl` is included, so a QEMU with virgl 3D (`-device virtio-gpu-gl`) gets host-GPU acceleration.

## T6: Buildroot image (Mesa 26.0.1, softpipe) + `make gl-boot-test` in CI

- Run [36149735380](https://github.com/Mudher84/wana-os/actions/runs/36149735380), commit `91d10a3`, job `toolchain, image, UEFI boot` (29m26s).
  `Build image` took 15m48s, including Mesa 26.0.1 and its host tools built from source for the first time.
- The earlier tests still pass in the same job: kernel config, kernel alone, initramfs, disk through GRUB, Phase 7 `wana-kms` graphics
- `make gl-boot-test` (KVM, OVMF → GRUB → kernel → ext4 → wana-init → wana-gl, virtio-gpu, Buildroot Mesa), 16 s:
  ```
  [BOOT] info: found: \[GBM\] info: surface [0-9]+x[0-9]+ XRGB8888
  [BOOT] info: found: \[EGL\] info: EGL 1\.[0-9]+
  [BOOT] info: found: \[EGL\] info: OpenGL ES context current
  [BOOT] info: found: \[RENDER\] info: GL_VENDOR=
  [BOOT] info: found: \[RENDER\] info: shaders compiled and linked
  [BOOT] info: found: \[DRM\] info: modeset done: .* first GPU frame on screen
  [BOOT] info: found: \[RENDER\] info: [0-9]+ frames rendered
  [BOOT] info: found: \[INIT\] info: /usr/bin/wana-gl exited successfully
  [BOOT] info: found: reboot: Power down
  [BOOT] info: pixel (640,400) = #4f8cff expected #4f8cff
  [BOOT] info: pixel (128,400) = #16213e expected #16213e
  [BOOT] info: pixel (0,0) = #ffffff expected #ffffff
  [BOOT] graphics test: PASS
  ```
- `ci` workflow (fmt, clippy, tests, MSRV with libgbm/libegl/libgles dev packages): success
- Result: **PASS**
- Gap in the evidence: the exact `GL_RENDERER`/`GL_VERSION` strings of the Buildroot Mesa are in the guest log
  artifact only (not downloadable from the development sandbox). softpipe is the only software rasterizer in the
  image and virgl needs QEMU virgl, which the runner does not use, so the renderer should be softpipe. This is not yet
  shown. Fix: `qemu-graphics-test.py` now echoes the guest's tagged lines to the console, so the next CI run
  prints the renderer.

## Phase 8 status

**PASS.** T1-T6 all pass. The frame is drawn by an OpenGL ES shader through Mesa (GBM + EGL) on the
Buildroot-built image and scanned out by Wana's own DRM/KMS code. It is verified pixel by pixel.

**Milestone 1 is complete:** GitHub repository → reproducible build → Linux kernel → minimal rootfs
(wana-init) → UEFI boot through GRUB → DRM/KMS → GBM/EGL/GLES → native graphical output.
