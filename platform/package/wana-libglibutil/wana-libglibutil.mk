################################################################################
#
# wana-libglibutil
#
################################################################################

WANA_LIBGLIBUTIL_VERSION = cccc4aa8f1745096f6feb66da7883b35055d9423
WANA_LIBGLIBUTIL_SITE = https://github.com/sailfishos/libglibutil.git
WANA_LIBGLIBUTIL_SITE_METHOD = git
WANA_LIBGLIBUTIL_LICENSE = BSD-3-Clause
WANA_LIBGLIBUTIL_LICENSE_FILES = LICENSE
WANA_LIBGLIBUTIL_INSTALL_STAGING = YES
WANA_LIBGLIBUTIL_DEPENDENCIES = host-pkgconf libglib2

WANA_LIBGLIBUTIL_MAKE_ENV = $(TARGET_CONFIGURE_OPTS)

define WANA_LIBGLIBUTIL_BUILD_CMDS
	$(WANA_LIBGLIBUTIL_MAKE_ENV) $(MAKE) -C $(@D) release pkgconfig
endef

define WANA_LIBGLIBUTIL_INSTALL_STAGING_CMDS
	$(WANA_LIBGLIBUTIL_MAKE_ENV) $(MAKE) -C $(@D) \
		DESTDIR="$(STAGING_DIR)" LIBDIR=usr/lib install-dev
endef

define WANA_LIBGLIBUTIL_INSTALL_TARGET_CMDS
	$(WANA_LIBGLIBUTIL_MAKE_ENV) $(MAKE) -C $(@D) \
		DESTDIR="$(TARGET_DIR)" LIBDIR=usr/lib install
endef

$(eval $(generic-package))
