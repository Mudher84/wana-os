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
	kernel-config-check kernel-boot-test br-%

help:
	@echo "Wana OS build targets:"
	@echo "  Rust / repository"
	@echo "    make check          fmt-check + lint + test + repo-check (run before every push)"
	@echo "    make fmt            format Rust code"
	@echo "    make lint           clippy, warnings are errors"
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

br-%: buildroot-src
	$(BR_MAKE) $*

clean:
	$(CARGO) clean
	rm -rf out/build

distclean: clean
	rm -rf out dl
