################################################################################
#
# wana-drm
#
################################################################################

# Built from this repository's Cargo workspace (local source, no download).
WANA_DRM_VERSION = 0.1.0
WANA_DRM_SITE = $(BR2_EXTERNAL_WANA_PATH)/..
WANA_DRM_SITE_METHOD = local
WANA_DRM_LICENSE = GPL-2.0+
WANA_DRM_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = \
	--exclude /out --exclude /target --exclude /dl --exclude /.git \
	--exclude /rust-toolchain.toml

WANA_DRM_CARGO_BUILD_OPTS = -p wana-drm

define WANA_DRM_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 \
		$(@D)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/wana-kms \
		$(TARGET_DIR)/usr/bin/wana-kms
endef

$(eval $(cargo-package))
