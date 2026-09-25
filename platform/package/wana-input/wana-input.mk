################################################################################
#
# wana-input
#
################################################################################

# Built from this repository's Cargo workspace (local source, no download).
WANA_INPUT_VERSION = 0.1.0
WANA_INPUT_SITE = $(BR2_EXTERNAL_WANA_PATH)/..
WANA_INPUT_SITE_METHOD = local
WANA_INPUT_LICENSE = GPL-2.0+
WANA_INPUT_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = \
	--exclude /out --exclude /target --exclude /dl --exclude /.git \
	--exclude /rust-toolchain.toml
# Links libudev, libinput and libxkbcommon from the target sysroot;
# xkeyboard-config provides the keymap data read at run time.
WANA_INPUT_DEPENDENCIES = udev libinput libxkbcommon xkeyboard-config

WANA_INPUT_CARGO_BUILD_OPTS = -p wana-input

define WANA_INPUT_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 \
		$(@D)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/wana-input \
		$(TARGET_DIR)/usr/bin/wana-input
endef

$(eval $(cargo-package))
