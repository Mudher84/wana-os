# Wana OS top-level build entry point. `make help` lists targets.
# Build output goes to out/ and dl/ (git-ignored); Rust output goes to target/.

CARGO ?= cargo

include platform/buildroot.env

BR_SRC     := $(CURDIR)/out/buildroot-$(BUILDROOT_VERSION)
BR_OUT     := $(CURDIR)/out/build/wana_x86_64
BR_EXTERNAL := $(CURDIR)/platform
# Source tarball cache, shared across builds and cached by CI.
export BR2_DL_DIR ?= $(CURDIR)/dl

BR_MAKE := $(MAKE) -C $(BR_SRC) O=$(BR_OUT) BR2_EXTERNAL=$(BR_EXTERNAL)

.PHONY: help check fmt fmt-check lint test repo-check clean distclean \
	buildroot-src config config-check savedefconfig toolchain kernel \
	kernel-config-check kernel-boot-test image manifest repro-compare msrv system-boot-test disk-boot-test graphics-boot-test gl-boot-test input-boot-test compositor-boot-test window-boot-test seat-boot-test text-boot-test text-window-boot-test layer-boot-test shell-boot-test wayland-host-test fonts br-%

help:
	@echo "Wana OS build targets:"
	@echo "  Rust / repository"
	@echo "    make check          fmt-check + lint + test + repo-check (run before every push)"
	@echo "    make fmt            format Rust code"
	@echo "    make lint           clippy, warnings are errors"
	@echo "    make msrv           build + test with Buildroot's Rust ($(RUST_MSRV))"
	@echo "    make test           unit tests (fetches the pinned fonts first: make fonts)"
	@echo "    make repo-check     repository hygiene"
	@echo "  System image (Buildroot $(BUILDROOT_VERSION))"
	@echo "    make buildroot-src  fetch and verify pinned Buildroot into out/"
	@echo "    make config         load wana_x86_64_defconfig into $(BR_OUT)"
	@echo "    make config-check   verify the defconfig loads and round-trips unchanged"
	@echo "    make savedefconfig  write the current config back to platform/configs/"
	@echo "    make toolchain      build the cross toolchain (needs network, ~30 min)"
	@echo "    make kernel         build the Linux kernel (bzImage) with the Wana fragment"
	@echo "    make kernel-config-check  verify every fragment option reached the kernel .config"
	@echo "    make kernel-boot-test     boot bzImage under QEMU+OVMF, check the serial log"
	@echo "    make image          full build (toolchain, kernel, Wana packages, rootfs, disk.img) + manifest"
	@echo "    make manifest       write images/build-manifest.json and images/SHA256SUMS"
	@echo "    make repro-compare A=<manifest> B=<manifest>  compare two builds artifact by artifact"
	@echo "    make system-boot-test  boot kernel + rootfs under QEMU+OVMF; wana-init must reach ready"
	@echo "    make disk-boot-test    boot images/disk.img via firmware -> GRUB -> kernel -> ext4 root"
	@echo "    make graphics-boot-test  boot disk.img with virtio-gpu, run wana-kms, check screenshot pixels"
	@echo "    make gl-boot-test        boot disk.img with virtio-gpu, run wana-gl (GBM/EGL/GLES), check pixels"
	@echo "    make input-boot-test     boot disk.img, inject keys + mouse via QEMU, wana-input must see them"
	@echo "    make compositor-boot-test  boot disk.img, wayland-info must list wana-compositor globals"
	@echo "    make window-boot-test    boot disk.img, a client window must appear (screenshot pixel check)"
	@echo "    make seat-boot-test      boot disk.img, QEMU click raises + focuses a window, typed keys reach it"
	@echo "    make wayland-host-test   headless compositor + test client on this host (3 protocol scenarios)"
	@echo "    make text-boot-test      boot disk.img, wana-text: pinned fonts, Arabic shaping, BiDi, layout"
	@echo "    make text-window-boot-test  boot disk.img, Arabic text drawn by wana-text in a window (hash + screenshot)"
	@echo "    make layer-boot-test     boot disk.img, the shell maps a background and a top bar; a window goes below the bar"
	@echo "    make shell-boot-test     boot disk.img, wana-shell: desktop + Arabic top bar, autostarted app below it"
	@echo "    make br-<target>    run any Buildroot target, e.g. make br-menuconfig"
	@echo "  make clean           remove Rust output and out/build/"
	@echo "  make distclean       also remove out/ and dl/"

check: fmt-check lint test repo-check

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

lint:
	$(CARGO) clippy --workspace --all-targets --locked -- -D warnings

# Unit tests of wana-text shape with the pinned fonts (same files as the image).
fonts:
	tools/fetch-fonts.sh

test: fonts
	$(CARGO) test --workspace --locked

repo-check:
	tools/check-repo.sh

# Buildroot compiles Wana crates with its own rustc; keep the code building there.
RUST_MSRV := $(shell sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml)
msrv: fonts
	$(CARGO) +$(RUST_MSRV) test --workspace --locked

buildroot-src:
	tools/fetch-buildroot.sh $(BR_SRC)

config: buildroot-src
	$(BR_MAKE) wana_x86_64_defconfig

config-check: buildroot-src
	tools/check-defconfig.sh $(BR_MAKE)

# Refuses when the defconfig was edited after the last `make config`:
# saving would silently overwrite those edits with the stale .config.
savedefconfig: buildroot-src
	@if [ ! -f $(BR_OUT)/.config ] || [ $(BR_EXTERNAL)/configs/wana_x86_64_defconfig -nt $(BR_OUT)/.config ]; then \
		echo "[CHECK] error: defconfig is newer than $(BR_OUT)/.config; run 'make config' first" >&2; exit 1; fi
	$(BR_MAKE) savedefconfig BR2_DEFCONFIG=$(BR_EXTERNAL)/configs/wana_x86_64_defconfig

toolchain: config
	$(BR_MAKE) toolchain

KERNEL_VERSION := $(shell sed -n 's/^BR2_LINUX_KERNEL_CUSTOM_VERSION_VALUE="\(.*\)"/\1/p' $(BR_EXTERNAL)/configs/wana_x86_64_defconfig)

kernel: config
	$(BR_MAKE) linux

kernel-config-check:
	tools/check-kernel-config.sh $(BR_EXTERNAL)/board/x86_64/linux.fragment \
		$(BR_OUT)/build/linux-$(KERNEL_VERSION)/.config

# Phase 3 has no root filesystem yet: success = the kernel boots via UEFI,
# initializes, and stops exactly where it looks for a root filesystem.
kernel-boot-test:
	mkdir -p out/logs
	tools/qemu-boot-test.sh --kernel $(BR_OUT)/images/bzImage \
		--log out/logs/kernel-boot.log --timeout 180 \
		--expect 'Linux version $(subst .,\.,$(KERNEL_VERSION))-wana' \
		--expect 'efi: EFI v[0-9]' \
		--expect 'VFS: Unable to mount root fs'

image: config
	$(BR_MAKE)
	$(MAKE) manifest

# Traceability: which commit, configs, versions produced this image, and the
# SHA-256 of every artifact (Phase 6).
manifest:
	mkdir -p out
	$(BR_MAKE) -s --no-print-directory show-info > out/show-info.json
	tools/build-manifest.py --br-out $(BR_OUT) --show-info out/show-info.json \
		--source-date-epoch "$$($(BR_MAKE) -s --no-print-directory printvars VARS=SOURCE_DATE_EPOCH | sed -n 's/^SOURCE_DATE_EPOCH=//p')"

repro-compare:
	tools/compare-manifests.py $(A) $(B)

# Phase 4: kernel + initramfs; wana-init must reach ready and power off.
system-boot-test:
	mkdir -p out/logs
	tools/qemu-boot-test.sh --kernel $(BR_OUT)/images/bzImage \
		--initrd $(BR_OUT)/images/rootfs.cpio.zst \
		--append "wana.test=poweroff" \
		--log out/logs/system-boot.log --timeout 180 \
		--expect 'Linux version $(subst .,\.,$(KERNEL_VERSION))-wana' \
		--expect 'Run /init as init process' \
		--expect '\[INIT\] info: wana-init [0-9.]+ starting' \
		--expect '\[INIT\] info: early mounts: 7 ok, 0 failed' \
		--expect '\[INIT\] info: hostname: wana' \
		--expect '\[INIT\] info: ready' \
		--expect 'reboot: Power down'

# Phase 5: the full disk image, booted the way hardware boots it:
# OVMF -> GRUB (ESP) -> kernel -> ext4 root by PARTUUID -> /sbin/init.
disk-boot-test:
	mkdir -p out/logs out/test
	. $(BR_EXTERNAL)/board/x86_64/disk.env; \
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-test.img "wana.test=poweroff" && \
	tools/qemu-boot-test.sh --disk out/test/disk-test.img \
		--log out/logs/disk-boot.log --timeout 180 \
		--expect 'BdsDxe: starting Boot' \
		--expect '\[BOOT\] info: loading Wana OS kernel' \
		--expect 'Linux version $(subst .,\.,$(KERNEL_VERSION))-wana' \
		--expect "root=PARTUUID=$$WANA_ROOT_PARTUUID" \
		--expect 'Run /sbin/init as init process' \
		--expect '\[INIT\] info: early mounts: 7 ok, 0 failed' \
		--expect '\[INIT\] info: ready' \
		--expect 'reboot: Power down'

# Phase 7: native graphical output. Boots the disk image with a virtio-gpu
# display, runs wana-kms (DRM/KMS modeset + page flips), screenshots the
# virtual screen while the frame is held and checks the test-pattern pixels.
GFX_ARGS := wana.run=/usr/bin/wana-kms,--hold,5 wana.test=poweroff wana.shell=0
graphics-boot-test:
	mkdir -p out/logs out/test
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-gfx.img "$(GFX_ARGS)"
	tools/qemu-graphics-test.py --disk out/test/disk-gfx.img --gpu virtio --timeout 180 \
		--log out/logs/graphics-boot.log \
		--screendump-on 'holding frame' --screendump out/test/wana-kms.ppm \
		--pixel 0.5,0.5=4f8cff --pixel 0.1,0.5=16213e --pixel 0,0=ffffff \
		--expect '\[DRM\] info: found card[0-9]+' \
		--expect '\[DRM\] info: selected .* on CRTC [0-9]+' \
		--expect '\[DRM\] info: modeset done' \
		--expect '\[DRM\] info: page flip: [0-9]+ flips completed' \
		--expect '\[INIT\] info: /usr/bin/wana-kms exited successfully' \
		--expect 'reboot: Power down'

# Phase 8: GPU rendering. Same boot path as graphics-boot-test, but the frame
# is drawn by an OpenGL ES shader through GBM + EGL (Mesa) and scanned out
# by wana-drm. The scene uses the same palette, so the same pixels are checked.
GL_MEMORY ?= 1024
GL_ARGS := wana.run=/usr/bin/wana-gl,--frames,10,--hold,5 wana.test=poweroff wana.shell=0
gl-boot-test:
	mkdir -p out/logs out/test
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-gl.img "$(GL_ARGS)"
	tools/qemu-graphics-test.py --disk out/test/disk-gl.img --gpu virtio --timeout 300 --memory $(GL_MEMORY) \
		--log out/logs/gl-boot.log \
		--screendump-on 'holding frame' --screendump out/test/wana-gl.ppm \
		--pixel 0.5,0.5=4f8cff --pixel 0.1,0.5=16213e --pixel 0,0=ffffff \
		--expect '\[GBM\] info: surface [0-9]+x[0-9]+ XRGB8888' \
		--expect '\[EGL\] info: EGL 1\.[0-9]+' \
		--expect '\[EGL\] info: OpenGL ES context current' \
		--expect '\[RENDER\] info: GL_VENDOR=' \
		--expect '\[RENDER\] info: shaders compiled and linked' \
		--expect '\[DRM\] info: modeset done: .* first GPU frame on screen' \
		--expect '\[RENDER\] info: [0-9]+ frames rendered' \
		--expect '\[INIT\] info: /usr/bin/wana-gl exited successfully' \
		--expect 'reboot: Power down'

# Phase 9: input. Boots disk.img with a virtio keyboard + tablet (plus the
# q35 PS/2 ones); wana-init starts udevd and coldplugs, then wana-input
# (udev -> libinput -> xkbcommon) waits for input. The QEMU monitor types
# "wana", moves the mouse and clicks; wana-input exits 0 only if all of it
# arrived, decoded with the US layout.
INPUT_ARGS := wana.run=/usr/bin/wana-input,--timeout,60,--expect-text,wana,--expect-pointer,--expect-button wana.test=poweroff wana.shell=0
input-boot-test:
	mkdir -p out/logs out/test
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-input.img "$(INPUT_ARGS)"
	tools/qemu-graphics-test.py --disk out/test/disk-input.img --gpu virtio --input virtio --timeout 180 \
		--log out/logs/input-boot.log \
		--send-on '\[INPUT\] info: waiting for input' \
		--send 'sendkey w' --send 'sendkey a' --send 'sendkey n' --send 'sendkey a' \
		--send 'mouse_move 40 30' --send 'mouse_button 1' --send 'mouse_button 0' \
		--expect '\[INIT\] info: udev: .*udevd started \(pid [0-9]+\)' \
		--expect '\[INIT\] info: udev: coldplug done in [0-9]+ ms: [0-9]+ devices initialized, [1-9][0-9]* input' \
		--expect '\[INPUT\] info: keymap compiled: English \(US\)' \
		--expect '\[INPUT\] info: device added: QEMU Virtio Keyboard .*\[keyboard\]' \
		--expect '\[INPUT\] info: device added: QEMU Virtio Tablet .*\[pointer\]' \
		--expect '\[INPUT\] info: typed text "wana" matches' \
		--expect '\[INPUT\] info: done: 4 key presses, [1-9][0-9]* pointer events, 1 left clicks' \
		--expect '\[INIT\] info: /usr/bin/wana-input exited successfully' \
		--expect 'reboot: Power down' \
		--reject 'Unknown (group|user)' --reject '\[(INIT|INPUT)\] error'

# Phase 10 steps 1-2: the Wayland protocol layer and its globals.
# wana-compositor opens its socket (libwayland-server, generated tables),
# describes the virtio-gpu display found through wana-drm as wl_output, and
# serves one real client, wayland-info, which must list every global with
# its contents (shm formats, output mode, seat name).
COMPOSITOR_ARGS := wana.run=/usr/bin/wana-compositor,--timeout,60,--shell,/usr/bin/wana-wl-test,--shell-arg,--expect-global,--shell-arg,zwlr_layer_shell_v1,--run,/usr/bin/wayland-info wana.test=poweroff wana.shell=0
compositor-boot-test:
	mkdir -p out/logs out/test
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-compositor.img "$(COMPOSITOR_ARGS)"
	tools/qemu-graphics-test.py --disk out/test/disk-compositor.img --gpu virtio --timeout 180 \
		--log out/logs/compositor-boot.log \
		--expect '\[COMPOSITOR\] info: protocol tables: [0-9]+ core \+ 5 xdg-shell interfaces' \
		--expect '\[COMPOSITOR\] info: globals: wl_compositor v4, wl_shm v1, wl_output v4, wl_seat v7, xdg_wm_base v1' \
		--expect '\[COMPOSITOR\] info: wl_output Virtual-1: [0-9]+x[0-9]+@[0-9.]+ Hz' \
		--expect '\[COMPOSITOR\] info: listening on /run/user/0/wayland-0 ' \
		--expect '\[COMPOSITOR\] info: client connected: pid [0-9]+ uid 0 gid 0' \
		--expect "interface: 'wl_compositor', +version: +4," \
		--expect "interface: 'wl_shm', +version: +1," \
		--expect "0 = 'AR24'" --expect "1 = 'XR24'" \
		--expect "interface: 'wl_output', +version: +4," \
		--expect 'name: Virtual-1' \
		--expect 'width: [0-9]+ px, height: [0-9]+ px, refresh: [0-9.]+ Hz,' \
		--expect 'flags: current preferred' \
		--expect "interface: 'wl_seat', +version: +7," \
		--expect 'name: seat0' \
		--expect "interface: 'xdg_wm_base', +version: +1," \
		--expect '\[COMPOSITOR\] info: test client /usr/bin/wayland-info exited successfully' \
		--expect '\[COMPOSITOR\] info: binds: wl_compositor 0, wl_shm 1, wl_output 1, wl_seat 1, xdg_wm_base 0, zwlr_layer_shell_v1 0' \
		--expect '\[COMPOSITOR\] info: privileged globals \(shell only\): zwlr_layer_shell_v1 v4' \
		--expect '\[COMPOSITOR\] info: shell: started /usr/bin/wana-wl-test .* on a private connection' \
		--expect '\[COMPOSITOR\] info: client connected: the shell \(private connection\)' \
		--expect '\[COMPOSITOR\] info: client: global zwlr_layer_shell_v1 v4 visible, as expected' \
		--expect '\[COMPOSITOR\] info: shell exited successfully' \
		--expect '\[COMPOSITOR\] info: shut down; socket removed' \
		--expect '\[INIT\] info: /usr/bin/wana-compositor exited successfully' \
		--expect 'reboot: Power down' \
		--reject "interface: 'zwlr_layer_shell_v1'" \
		--reject '\[(INIT|COMPOSITOR|DRM)\] (warn|error)'

# Phase 10 step 3: a client window on screen. wana-compositor draws with
# GLES on the virtio-gpu display; wana-wl-test creates an xdg_toplevel,
# does the configure handshake and commits a 480x320 XRGB8888 wl_shm buffer
# (accent + 8 px white border). Once its frame callback reports the frame
# presented, the screen is captured and the window's pixels checked:
# centered at 400,240 on 1280x800 (center accent, left/top border white,
# outside = compositor background).
WINDOW_ARGS := wana.run=/usr/bin/wana-compositor,--timeout,120,--run,/usr/bin/wana-wl-test,--hold,5 wana.test=poweroff wana.shell=0
window-boot-test:
	mkdir -p out/logs out/test
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-window.img "$(WINDOW_ARGS)"
	tools/qemu-graphics-test.py --disk out/test/disk-window.img --gpu virtio --timeout 240 --memory 1024 \
		--log out/logs/window-boot.log \
		--screendump-on 'client: holding window' --screendump out/test/window.ppm \
		--pixel 0.5,0.5=4f8cff --pixel 0.3141,0.5=ffffff --pixel 0.5,0.30375=ffffff \
		--pixel 0.1,0.5=16213e --pixel 0,0=16213e \
		--expect '\[RENDER\] info: compositor renderer: GL_RENDERER=' \
		--expect '\[DRM\] info: modeset done: .* compositor frame on screen' \
		--expect '\[COMPOSITOR\] info: client: configure received \(serial [0-9]+\); acking' \
		--expect '\[COMPOSITOR\] info: window mapped: "wana-wl-test" \(org.wana.test\) 480x320 at 400,240' \
		--expect '\[COMPOSITOR\] info: frame [0-9]+: 1 window\(s\) on screen' \
		--expect '\[COMPOSITOR\] info: client: frame presented \(callback done at [0-9]+ ms\); buffer released' \
		--expect '\[COMPOSITOR\] info: test client /usr/bin/wana-wl-test exited successfully' \
		--expect '\[INIT\] info: /usr/bin/wana-compositor exited successfully' \
		--expect 'reboot: Power down' \
		--reject '\[(INIT|COMPOSITOR|DRM|RENDER)\] (warn|error)'

# Phase 10 step 4: input, focus and z-order through the compositor.
# wana-wl-test --zorder maps "back" (larger than the screen, placed at 0,0,
# amber) and then "front" (480x320, accent) on top, which takes keyboard
# focus. QEMU drives the pointer into the top-left corner (large relative
# moves; the cursor clamps at the edge, so the position does not depend on
# pointer acceleration), where only "back" is visible, clicks, and types
# Shift+W a n a. The click must raise "back" and move keyboard focus to it
# (the screenshot's center turns amber), and the client must decode "Wana"
# with the compositor's keymap (the capital W proves wl_keyboard.modifiers).
# The client sets a 64x64 magenta cursor surface (hotspot 0,0) on every
# pointer enter; the pointer ends within 60 px of the corner, so pixel
# (60,60) must be magenta (client cursor drawn instead of the arrow).
SEAT_ARGS := wana.run=/usr/bin/wana-compositor,--timeout,150,--run,/usr/bin/wana-wl-test,--zorder,Wana,--hold,3 wana.test=poweroff wana.shell=0
seat-boot-test:
	mkdir -p out/logs out/test
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-seat.img "$(SEAT_ARGS)"
	tools/qemu-graphics-test.py --disk out/test/disk-seat.img --gpu virtio --input virtio --timeout 300 --memory 1024 \
		--log out/logs/seat-boot.log \
		--send-on 'client: ready for input' \
		--send 'mouse_move -300 -300' --send 'mouse_move -300 -300' --send 'mouse_move -300 -300' \
		--send 'mouse_move -300 -300' --send 'mouse_move 10 10' \
		--send 'mouse_button 1' --send 'mouse_button 0' \
		--send 'sendkey shift-w' --send 'sendkey a' --send 'sendkey n' --send 'sendkey a' \
		--screendump-on 'client: input received' --screendump out/test/seat.ppm \
		--pixel 0.5,0.5=e0a030 --pixel 0.3,0.5=e0a030 --pixel 0.9,0.9=e0a030 --pixel 0.046875,0.075=ff00ff \
		--expect '\[INPUT\] info: keymap English \(US\) for clients: [0-9]+ bytes, sealed memfd' \
		--expect '\[INPUT\] info: seat0 capabilities: pointer, keyboard' \
		--expect '\[COMPOSITOR\] info: client: seat capabilities 0x3 \(pointer \+ keyboard\)' \
		--expect '\[COMPOSITOR\] info: client: keymap received: [0-9]+ bytes, layout English \(US\)' \
		--expect '\[COMPOSITOR\] info: window mapped: "wana-wl-test back" \(org.wana.test\) 1400x900 at 0,0' \
		--expect '\[COMPOSITOR\] info: window mapped: "wana-wl-test" \(org.wana.test\) 480x320 at 432,272' \
		--expect '\[COMPOSITOR\] info: client: ready for input \(keyboard focus on window "front", keymap compiled\)' \
		--expect '\[COMPOSITOR\] info: client: pointer entered window "back" at [0-9.]+,[0-9.]+' \
		--expect '\[COMPOSITOR\] info: cursor: client surface 64x64, hotspot 0,0 \(from "wana-wl-test back" \(org.wana.test\)\)' \
		--expect '\[COMPOSITOR\] info: window raised: "wana-wl-test back" \(org.wana.test\)' \
		--expect '\[COMPOSITOR\] info: client: keyboard focus left window "front"' \
		--expect '\[COMPOSITOR\] info: client: keyboard focus on window "back" \(0 key\(s\) held\)' \
		--expect '\[COMPOSITOR\] info: client: left click on window "back" at [0-9.]+,[0-9.]+' \
		--expect '\[COMPOSITOR\] info: client: key 42 pressed on window "back": Shift_L' \
		--expect '\[COMPOSITOR\] info: client: key 17 pressed on window "back": W text "W"' \
		--expect '\[COMPOSITOR\] info: client: input received: typed "Wana" on window "back", left click on window "back", [0-9]+ motion event\(s\)' \
		--expect '\[COMPOSITOR\] info: test client /usr/bin/wana-wl-test exited successfully' \
		--expect '\[INIT\] info: /usr/bin/wana-compositor exited successfully' \
		--expect 'reboot: Power down' \
		--reject '\[(INIT|COMPOSITOR|DRM|RENDER|INPUT)\] (warn|error)'

# Decision 0002, text steps 1-3, in the image:
# 1. the pinned UI fonts, verified against SHA256SUMS and loaded through
#    HarfBuzz (Arabic and Latin coverage required);
# 2. Arabic shaping (joined forms, lam-alef) and the BiDi levels and visual
#    order of a mixed line;
# 3. layout of a mixed line in a narrow width: line breaks, fallback fonts,
#    right alignment, carets.
# Glyph IDs and widths come from the pinned fonts. They were recorded with
# the host's HarfBuzz 8.3 and must be the same with the image's 12.3.
TEXT_ARGS := wana.run=/usr/bin/wana-text wana.test=poweroff wana.shell=0
text-boot-test:
	mkdir -p out/logs out/test
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-text.img "$(TEXT_ARGS)"
	tools/qemu-graphics-test.py --disk out/test/disk-text.img --gpu virtio --timeout 180 \
		--log out/logs/text-boot.log \
		--expect '\[RENDER\] info: font verified: NotoNaskhArabic-VF.ttf \(329920 bytes, sha256 02d9310b6b55b3bf8a5084fced9106ccd914650d730cbe8ff3b57f691d2931f6\)' \
		--expect '\[RENDER\] info: font verified: NotoSansArabic-VF.ttf \(844676 bytes, sha256 63111b5b2e074dd48cc67692e0a2726d86ee94c1c37fe8598257b7b4e87e869e\)' \
		--expect '\[RENDER\] info: font verified: NotoSans-VF.ttf \(2493792 bytes, sha256 e0890ec6da515d47b9d7cdb8b4ded1d9255fc4e5254ae03f9a579da6cb354717\)' \
		--expect '\[RENDER\] info: font loaded: NotoNaskhArabic-VF.ttf: "Noto Naskh Arabic", 1726 glyphs, 1000 units/em, axes \[wght 400..700 \(default 400\)\], covers \[Arabic, Latin\]' \
		--expect '\[RENDER\] info: font loaded: NotoSansArabic-VF.ttf: "Noto Sans Arabic", 1711 glyphs, 1000 units/em' \
		--expect '\[RENDER\] info: font loaded: NotoSans-VF.ttf: "Noto Sans", 4671 glyphs, 1000 units/em, .*covers \[Latin\]' \
		--expect '\[RENDER\] info: fonts ok: 3 verified and loaded, Arabic and Latin covered' \
		--expect '\[RENDER\] info: shaped "مرحبا" \(Noto Naskh Arabic, RTL\): 6 glyphs \[9 322 16 25 29 77\], width 2041 units, joined forms: yes' \
		--expect '\[RENDER\] info: shaped "لا": lam 71 \(initial lam 70\), alef 10 \(final alef 9\): lam-alef forms: yes' \
		--expect '\[RENDER\] info: bidi "Wana 2026 وانا" \(RTL paragraph\): levels 22222222211111, left to right: "وانا" RTL \| "Wana 2026" LTR' \
		--expect '\[RENDER\] info: text ok: fonts, Arabic shaping and BiDi \(FriBidi, UAX #9\)' \
		--expect '\[RENDER\] info: layout "مرحبا بك في Wana 2026" at 20px in 120px: 2 lines: "مرحبا بك في" 100.0px fonts \{1\} / "Wana 2026" 104.0px fonts \{0\}' \
		--expect '\[RENDER\] info: layout checks: fits true, right-aligned true, carets right to left true, clicks round-trip true' \
		--expect '\[RENDER\] info: text ok: layout \(line breaks, fallback fonts, alignment, carets\)' \
		--expect '\[INIT\] info: /usr/bin/wana-text exited successfully' \
		--expect 'reboot: Power down' \
		--reject '\[(INIT|RENDER)\] (warn|error)'

# Decision 0002, text step 4: the whole text stack on screen. wana-wl-test
# --text lays out Arabic + Latin with wana-text, rasterizes it into its
# window (deterministic: the SHA-256 of its pixels is fixed), and the
# compositor shows it. The screenshot checks a fully inked pixel reported
# by the client (window 324,48 -> screen 664,343) and the panel around it.
TEXT_SHA256 := 7e7cc3f09afa024bc2e3df715aae86f0cbe818d8684b7b4159196b8472342790
TEXT_WINDOW_ARGS := wana.run=/usr/bin/wana-compositor,--timeout,150,--run,/usr/bin/wana-wl-test,--text,--hold,3 wana.test=poweroff wana.shell=0
text-window-boot-test:
	mkdir -p out/logs out/test
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-textwin.img "$(TEXT_WINDOW_ARGS)"
	tools/qemu-graphics-test.py --disk out/test/disk-textwin.img --gpu virtio --timeout 240 --memory 1024 \
		--log out/logs/text-window-boot.log \
		--screendump-on 'client: holding window' --screendump out/test/text-window.ppm \
		--pixel 0.51875,0.42875=ffffff --pixel 0.26953125,0.375=243b6b --pixel 0,0=16213e \
		--expect '\[COMPOSITOR\] info: client: text rendered: 2 lines, 600x209, 8395 ink pixels, sha256 $(TEXT_SHA256)' \
		--expect '\[COMPOSITOR\] info: client: fully inked pixel at 324,48 \(window coordinates\)' \
		--expect '\[COMPOSITOR\] info: window mapped: "wana-wl-test text" \(org.wana.test\) 600x209 at 340,295' \
		--expect '\[COMPOSITOR\] info: client: frame presented' \
		--expect '\[COMPOSITOR\] info: test client /usr/bin/wana-wl-test exited successfully' \
		--expect '\[INIT\] info: /usr/bin/wana-compositor exited successfully' \
		--expect 'reboot: Power down' \
		--reject '\[(INIT|COMPOSITOR|DRM|RENDER)\] (warn|error)'

# Phase 11 shell step 2 (decision 0003): layer surfaces. wana-wl-test runs
# as the shell on the private connection: a background (all edges, zone -1)
# and a 40 px top bar (zone 40) through the configure handshake, then an
# ordinary window, which must be centered in what the bar leaves: 400,260.
# Pixels: the bar, the desktop left of and above the window, the window's
# top border and its interior.
LAYER_ARGS := wana.run=/usr/bin/wana-compositor,--timeout,150,--shell,/usr/bin/wana-wl-test,--shell-arg,--layers,--shell-arg,--hold,--shell-arg,3,--exit-with-shell wana.test=poweroff wana.shell=0
layer-boot-test:
	mkdir -p out/logs out/test
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-layer.img "$(LAYER_ARGS)"
	tools/qemu-graphics-test.py --disk out/test/disk-layer.img --gpu virtio --timeout 240 --memory 1024 \
		--log out/logs/layer-boot.log \
		--screendump-on 'client: holding window' --screendump out/test/layer.ppm \
		--pixel 0.5,0.025=0b0f1a --pixel 0.078125,0.5=1b3a5c --pixel 0.5,0.3125=1b3a5c \
		--pixel 0.5,0.3275=ffffff --pixel 0.5,0.525=4f8cff \
		--expect '\[COMPOSITOR\] info: client connected: the shell \(private connection\)' \
		--expect '\[COMPOSITOR\] info: client: layer "wana-desktop" configured 1280x800' \
		--expect '\[COMPOSITOR\] info: layer surface mapped: "wana-desktop" on layer background at 0,0 1280x800 \(exclusive zone -1\)' \
		--expect '\[COMPOSITOR\] info: client: layer "wana-bar" configured 1280x40' \
		--expect '\[COMPOSITOR\] info: usable area for windows: 0,40 1280x760' \
		--expect '\[COMPOSITOR\] info: layer surface mapped: "wana-bar" on layer top at 0,0 1280x40 \(exclusive zone 40\)' \
		--expect '\[COMPOSITOR\] info: window mapped: "wana-wl-test" \(org.wana.test\) 480x320 at 400,260' \
		--expect '\[COMPOSITOR\] info: shell exited successfully' \
		--expect '\[INIT\] info: /usr/bin/wana-compositor exited successfully' \
		--expect 'reboot: Power down' \
		--reject '\[(INIT|COMPOSITOR|DRM|RENDER)\] (warn|error)'

# Phase 11 shell step 3a: wana-shell itself. The compositor starts it on
# the private connection; it maps the desktop (gradient) and the top bar
# ("وانا" at the right, the time at the left in Arabic-Indic digits, fixed
# with --clock so the bar's pixels are compared by hash), then autostarts an
# ordinary client through the public socket, which must not inherit any
# descriptor and whose window goes below the bar.
SHELL_SHA_DESKTOP := c203532a708e005885a04bb854150ee8b4ed80eeef930d613d377756e9bf4839
SHELL_SHA_BAR := 1359d3bd16029f7e54f4b04c42f0a23df6c55d872ffea676aad584f10a39b5a8
SHELL_ARGS := wana.run=/usr/bin/wana-compositor,--timeout,150,--exit-with-shell,--shell,/usr/bin/wana-shell,--shell-arg,--clock,--shell-arg,16:20,--shell-arg,--autostart,--shell-arg,/usr/bin/wana-wl-test,--shell-arg,--autostart-arg,--shell-arg,--no-inherited-fds,--shell-arg,--autostart-arg,--shell-arg,--hold,--shell-arg,--autostart-arg,--shell-arg,3,--shell-arg,--exit-with-autostart wana.test=poweroff wana.shell=0
shell-boot-test:
	mkdir -p out/logs out/test
	tools/mk-test-disk.sh $(BR_OUT)/images/disk.img out/test/disk-shell.img "$(SHELL_ARGS)"
	tools/qemu-graphics-test.py --disk out/test/disk-shell.img --gpu virtio --timeout 240 --memory 1024 \
		--log out/logs/shell-boot.log \
		--screendump-on 'client: holding window' --screendump out/test/shell.ppm \
		--pixel 0.5,0.025=0b0f1a --pixel 0.5,0.3275=ffffff --pixel 0.5,0.525=4f8cff \
		--expect '\[SHELL\] info: wana-shell [0-9.]+ starting' \
		--expect '\[COMPOSITOR\] info: client connected: the shell \(private connection\)' \
		--expect '\[SHELL\] info: desktop mapped: 1280x800, sha256 $(SHELL_SHA_DESKTOP)' \
		--expect '\[SHELL\] info: bar mapped: 1280x40, time ١٦:٢٠, sha256 $(SHELL_SHA_BAR)' \
		--expect '\[COMPOSITOR\] info: usable area for windows: 0,40 1280x760' \
		--expect '\[SHELL\] info: ready' \
		--expect '\[SHELL\] info: autostart: /usr/bin/wana-wl-test --no-inherited-fds --hold 3' \
		--expect '\[COMPOSITOR\] info: client: inherited descriptors: 0 1 2 only' \
		--expect '\[COMPOSITOR\] info: window mapped: "wana-wl-test" \(org.wana.test\) 480x320 at 400,260' \
		--expect '\[SHELL\] info: autostart exited successfully' \
		--expect '\[COMPOSITOR\] info: shell exited successfully' \
		--expect '\[INIT\] info: /usr/bin/wana-compositor exited successfully' \
		--expect 'reboot: Power down' \
		--reject '\[(INIT|COMPOSITOR|DRM|RENDER|SHELL)\] (warn|error)'

# Phase 10 step 3 on the build host, headless (no display needed): the
# same client in its three scenarios against the real libwayland.
WAYLAND_HOST_RUN = env -u WAYLAND_DISPLAY XDG_RUNTIME_DIR=$$dir target/release/wana-compositor --timeout 30 --headless 1280x800@60 --run target/release/wana-wl-test
# The shell alone, the compositor stopping with it (layer surfaces).
WAYLAND_HOST_LAYERS = env -u WAYLAND_DISPLAY XDG_RUNTIME_DIR=$$dir target/release/wana-compositor --timeout 30 --headless 1280x800@60 \
	--exit-with-shell --shell target/release/wana-wl-test --shell-arg
# The same compositor with a shell on the private connection (decision 0003).
WAYLAND_HOST_SHELL = env -u WAYLAND_DISPLAY XDG_RUNTIME_DIR=$$dir target/release/wana-compositor --timeout 30 --headless 1280x800@60 \
	--shell target/release/wana-wl-test --shell-arg --expect-global --shell-arg zwlr_layer_shell_v1
wayland-host-test: fonts
	$(CARGO) build --release --locked -p wana-compositor -p wana-wl-test -p wana-shell
	@dir=$$(mktemp -d) && trap 'rm -rf "$$dir"' EXIT && \
	$(WAYLAND_HOST_RUN) > $$dir/window.log 2>&1 && grep -q 'window mapped: "wana-wl-test"' $$dir/window.log && \
		grep -q 'client: frame presented' $$dir/window.log && echo "[COMPOSITOR] check: window scenario: PASS" && \
	$(WAYLAND_HOST_RUN) --attach-before-configure > $$dir/early.log 2>&1 && \
		grep -q 'client: got the expected protocol error: xdg_surface@[0-9]* code 3' $$dir/early.log && \
		echo "[COMPOSITOR] check: buffer before configure -> xdg_surface.unconfigured_buffer: PASS" && \
	$(WAYLAND_HOST_RUN) --truncate-pool > $$dir/sigbus.log 2>&1 && \
		grep -q 'client: got the expected protocol error: wl_buffer@[0-9]* code 2' $$dir/sigbus.log && \
		grep -q 'shut down; socket removed' $$dir/sigbus.log && \
		echo "[COMPOSITOR] check: truncated pool (SIGBUS) -> wl_shm.invalid_fd, compositor survives: PASS" && \
	$(WAYLAND_HOST_RUN) --text --fonts $(CURDIR)/out/fonts > $$dir/text.log 2>&1 && \
		grep -q 'client: text rendered: 2 lines, 600x209, 8395 ink pixels, sha256 $(TEXT_SHA256)' $$dir/text.log && \
		echo "[COMPOSITOR] check: Arabic text window, rendering sha256 as in the image: PASS" && \
	$(WAYLAND_HOST_SHELL) --run target/release/wana-wl-test --expect-no-global zwlr_layer_shell_v1 --expect-global xdg_wm_base > $$dir/priv.log 2>&1 && \
		grep -q 'client: global zwlr_layer_shell_v1 v4 visible, as expected' $$dir/priv.log && \
		grep -q 'client: global zwlr_layer_shell_v1 not visible, as expected' $$dir/priv.log && \
		echo "[COMPOSITOR] check: layer-shell visible to the shell only: PASS" && \
	$(WAYLAND_HOST_SHELL) --run target/release/wana-wl-test --try-bind-hidden zwlr_layer_shell_v1 > $$dir/bind.log 2>&1 && \
		grep -q 'client: got the expected protocol error: wl_registry@[0-9]* code 0' $$dir/bind.log && \
		grep -q 'binds: .*zwlr_layer_shell_v1 0' $$dir/bind.log && \
		echo "[COMPOSITOR] check: binding the hidden global by guessing its name -> invalid_object: PASS" && \
	$(WAYLAND_HOST_LAYERS) --layers > $$dir/layers.log 2>&1 && \
		grep -q 'usable area for windows: 0,40 1280x760' $$dir/layers.log && \
		grep -q 'window mapped: "wana-wl-test" (org.wana.test) 480x320 at 400,260' $$dir/layers.log && \
		echo "[COMPOSITOR] check: background + bar layer surfaces, window placed below the bar: PASS" && \
	$(WAYLAND_HOST_LAYERS) --layer-invalid-size > $$dir/badlayer.log 2>&1 && \
		grep -q 'client: got the expected protocol error: zwlr_layer_surface_v1@[0-9]* code 1' $$dir/badlayer.log && \
		echo "[COMPOSITOR] check: layer width 0 without both side anchors -> invalid_size: PASS" && \
	env -u WAYLAND_DISPLAY XDG_RUNTIME_DIR=$$dir target/release/wana-compositor --timeout 30 --headless 1280x800@60 --exit-with-shell \
		--shell target/release/wana-shell --shell-arg --fonts --shell-arg $(CURDIR)/out/fonts --shell-arg --clock --shell-arg 16:20 \
		--shell-arg --autostart --shell-arg $(CURDIR)/target/release/wana-wl-test --shell-arg --autostart-arg --shell-arg --no-inherited-fds \
		--shell-arg --exit-with-autostart \
		> $$dir/shell.log 2>&1 && \
		grep -q 'desktop mapped: 1280x800, sha256 $(SHELL_SHA_DESKTOP)' $$dir/shell.log && \
		grep -q 'bar mapped: 1280x40, time ١٦:٢٠, sha256 $(SHELL_SHA_BAR)' $$dir/shell.log && \
		grep -q 'client: inherited descriptors: 0 1 2 only' $$dir/shell.log && \
		grep -q 'window mapped: "wana-wl-test" (org.wana.test) 480x320 at 400,260' $$dir/shell.log && \
		echo "[COMPOSITOR] check: wana-shell desktop + bar (hashes), autostarted app below the bar, no inherited fds: PASS" || \
	{ echo "[COMPOSITOR] check: FAIL" >&2; tail -n 15 $$dir/*.log >&2; exit 1; }

br-%: buildroot-src
	$(BR_MAKE) $*

clean:
	$(CARGO) clean
	rm -rf out/build

distclean: clean
	rm -rf out dl
