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
	kernel-config-check kernel-boot-test image manifest repro-compare msrv system-boot-test disk-boot-test br-%

help:
	@echo "Wana OS build targets:"
	@echo "  Rust / repository"
	@echo "    make check          fmt-check + lint + test + repo-check (run before every push)"
	@echo "    make fmt            format Rust code"
	@echo "    make lint           clippy, warnings are errors"
	@echo "    make msrv           build + test with Buildroot's Rust ($(RUST_MSRV))"
	@echo "    make test           unit tests"
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

test:
	$(CARGO) test --workspace --locked

repo-check:
	tools/check-repo.sh

# Buildroot compiles Wana crates with its own rustc; keep the code building there.
RUST_MSRV := $(shell sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml)
msrv:
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

br-%: buildroot-src
	$(BR_MAKE) $*

clean:
	$(CARGO) clean
	rm -rf out/build

distclean: clean
	rm -rf out dl
