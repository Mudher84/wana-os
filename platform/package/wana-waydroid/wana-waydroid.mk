################################################################################
#
# wana-waydroid
#
################################################################################

WANA_WAYDROID_VERSION = 5b7e2e71be3f6bfaaaab3b461251dacaf1ce4991
WANA_WAYDROID_SITE = https://github.com/waydroid/waydroid.git
WANA_WAYDROID_SITE_METHOD = git
WANA_WAYDROID_LICENSE = GPL-3.0-or-later
WANA_WAYDROID_LICENSE_FILES = LICENSE
WANA_WAYDROID_DEPENDENCIES = \
	python3 dbus-python python-gobject libgtk3 polkit iptables dnsmasq iproute2 ca-certificates \
	lxc wana-python-gbinder

define WANA_WAYDROID_INSTALL_TARGET_CMDS
	$(TARGET_MAKE_ENV) $(MAKE) -C $(@D) install \
		DESTDIR="$(TARGET_DIR)" PREFIX=/usr SYSCONFDIR=/etc \
		USE_SYSTEMD=0 USE_DBUS_ACTIVATION=0 USE_NFTABLES=0
endef

$(eval $(generic-package))
