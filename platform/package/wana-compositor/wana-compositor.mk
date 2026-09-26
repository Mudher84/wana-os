################################################################################
#
# wana-compositor
#
################################################################################

# Built from this repository's Cargo workspace (local source, no download).
WANA_COMPOSITOR_VERSION = 0.1.0
WANA_COMPOSITOR_SITE = $(BR2_EXTERNAL_WANA_PATH)/..
WANA_COMPOSITOR_SITE_METHOD = local
WANA_COMPOSITOR_LICENSE = GPL-2.0+
WANA_COMPOSITOR_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = \
	--exclude /out --exclude /target --exclude /dl --exclude /.git \
	--exclude /rust-toolchain.toml
# Links libwayland-server (and, for wana-wl-test, libwayland-client) and
# Mesa's EGL/GLESv2/GBM, libudev/libinput/libxkbcommon for input, and
# (wana-wl-test --text) libharfbuzz/libfribidi with the pinned fonts,
# from the target sysroot. wana-wayland's build.rs
# generates the protocol tables from the XML these packages install in
# staging, so the tables match the exact library version in the image.
WANA_COMPOSITOR_DEPENDENCIES = wayland wayland-protocols libegl libgles libgbm \
	udev libinput libxkbcommon xkeyboard-config harfbuzz libfribidi wana-fonts
WANA_COMPOSITOR_CARGO_ENV = \
	WANA_WAYLAND_XML=$(STAGING_DIR)/usr/share/wayland/wayland.xml \
	WANA_WAYLAND_PROTOCOLS_DIR=$(STAGING_DIR)/usr/share/wayland-protocols

WANA_COMPOSITOR_CARGO_BUILD_OPTS = -p wana-compositor -p wana-shell -p wana-wl-test

WANA_COMPOSITOR_BIN_DIR = $(@D)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)

define WANA_COMPOSITOR_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 $(WANA_COMPOSITOR_BIN_DIR)/wana-compositor $(TARGET_DIR)/usr/bin/wana-compositor
	$(INSTALL) -D -m 0755 $(WANA_COMPOSITOR_BIN_DIR)/wana-shell $(TARGET_DIR)/usr/bin/wana-shell
	$(INSTALL) -D -m 0755 $(WANA_COMPOSITOR_BIN_DIR)/wana-wl-test $(TARGET_DIR)/usr/bin/wana-wl-test
endef

$(eval $(cargo-package))
