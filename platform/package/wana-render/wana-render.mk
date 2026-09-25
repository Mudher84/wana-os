################################################################################
#
# wana-render
#
################################################################################

# Built from this repository's Cargo workspace (local source, no download).
WANA_RENDER_VERSION = 0.1.0
WANA_RENDER_SITE = $(BR2_EXTERNAL_WANA_PATH)/..
WANA_RENDER_SITE_METHOD = local
WANA_RENDER_LICENSE = GPL-2.0+
WANA_RENDER_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = \
	--exclude /out --exclude /target --exclude /dl --exclude /.git \
	--exclude /rust-toolchain.toml
# Links libgbm, libEGL and libGLESv2 from the target sysroot.
WANA_RENDER_DEPENDENCIES = libegl libgles libgbm

WANA_RENDER_CARGO_BUILD_OPTS = -p wana-render

define WANA_RENDER_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 \
		$(@D)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/wana-gl \
		$(TARGET_DIR)/usr/bin/wana-gl
endef

$(eval $(cargo-package))
