################################################################################
#
# wana-init
#
################################################################################

# Built from this repository's Cargo workspace (local source, no download).
WANA_INIT_VERSION = 0.1.0
WANA_INIT_SITE = $(BR2_EXTERNAL_WANA_PATH)/..
WANA_INIT_SITE_METHOD = local
WANA_INIT_LICENSE = GPL-2.0+
# Do not copy build output or the Buildroot tree itself into the build dir.
WANA_INIT_OVERRIDE_SRCDIR_RSYNC_EXCLUSIONS = \
	--exclude /out --exclude /target --exclude /dl --exclude /.git \
	--exclude /rust-toolchain.toml

WANA_INIT_CARGO_BUILD_OPTS = -p wana-init

define WANA_INIT_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0755 \
		$(@D)/target/$(RUSTC_TARGET_NAME)/$(if $(BR2_ENABLE_DEBUG),debug,release)/wana-init \
		$(TARGET_DIR)/usr/sbin/wana-init
endef

$(eval $(cargo-package))
