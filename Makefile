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
	buildroot-src config config-check savedefconfig toolchain br-%

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

savedefconfig: buildroot-src
	$(BR_MAKE) savedefconfig BR2_DEFCONFIG=$(BR_EXTERNAL)/configs/wana_x86_64_defconfig

toolchain: config
	$(BR_MAKE) toolchain

br-%: buildroot-src
	$(BR_MAKE) $*

clean:
	$(CARGO) clean
	rm -rf out/build

distclean: clean
	rm -rf out dl
