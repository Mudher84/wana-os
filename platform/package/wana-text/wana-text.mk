################################################################################
#
# wana-text
#
################################################################################

# Built from this repository's Cargo workspace (local source, no download).
WANA_TEXT_VERSION = 0.1.0
WANA_TEXT_SITE = $(BR2_EXTERNAL_WANA_PATH)/..
WANA_TEXT_SITE_METHOD = local
WANA_TEXT_LICENSE = GPL-2.0+
WANA_TEXT_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = \
	--exclude /out --exclude /target --exclude /dl --exclude /.git \
	--exclude /rust-toolchain.toml
# Links libharfbuzz from the target sysroot; the fonts it checks at run
# time come from wana-fonts.
WANA_TEXT_DEPENDENCIES = harfbuzz wana-fonts

WANA_TEXT_CARGO_BUILD_OPTS = -p wana-text

define WANA_TEXT_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 \
		$(@D)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/wana-text \
		$(TARGET_DIR)/usr/bin/wana-text
endef

$(eval $(cargo-package))
