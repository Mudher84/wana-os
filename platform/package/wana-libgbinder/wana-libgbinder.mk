################################################################################
#
# wana-libgbinder
#
################################################################################

WANA_LIBGBINDER_VERSION = e906afcffbfa51b7fbefe042a13b933d9e8dfdd9
WANA_LIBGBINDER_SITE = https://github.com/mer-hybris/libgbinder.git
WANA_LIBGBINDER_SITE_METHOD = git
WANA_LIBGBINDER_LICENSE = BSD-3-Clause
WANA_LIBGBINDER_LICENSE_FILES = LICENSE
WANA_LIBGBINDER_INSTALL_STAGING = YES
WANA_LIBGBINDER_DEPENDENCIES = host-pkgconf libglib2 wana-libglibutil

WANA_LIBGBINDER_MAKE_ENV = \
	$(TARGET_MAKE_ENV) \
	CC="$(TARGET_CC)" \
	AR="$(TARGET_AR)" \
	RANLIB="$(TARGET_RANLIB)" \
	STRIP="$(TARGET_STRIP)" \
	PKG_CONFIG="$(PKG_CONFIG_HOST_BINARY)" \
	CFLAGS="$(TARGET_CFLAGS)" \
	LDFLAGS="$(TARGET_LDFLAGS)"

define WANA_LIBGBINDER_BUILD_CMDS
	$(WANA_LIBGBINDER_MAKE_ENV) $(MAKE) -C $(@D) release pkgconfig
endef

define WANA_LIBGBINDER_INSTALL_STAGING_CMDS
	$(WANA_LIBGBINDER_MAKE_ENV) $(MAKE) -C $(@D) \
		DESTDIR="$(STAGING_DIR)" LIBDIR=usr/lib install-dev
endef

define WANA_LIBGBINDER_INSTALL_TARGET_CMDS
	$(WANA_LIBGBINDER_MAKE_ENV) $(MAKE) -C $(@D) \
		DESTDIR="$(TARGET_DIR)" LIBDIR=usr/lib install
endef

$(eval $(generic-package))
