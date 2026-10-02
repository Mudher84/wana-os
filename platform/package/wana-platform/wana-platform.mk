################################################################################
#
# wana-platform
#
################################################################################

WANA_PLATFORM_VERSION = 0.1.0
WANA_PLATFORM_SITE = $(BR2_EXTERNAL_WANA_PATH)/package/wana-platform/files
WANA_PLATFORM_SITE_METHOD = local
WANA_PLATFORM_LICENSE = GPL-2.0-or-later
WANA_PLATFORM_DEPENDENCIES = dbus chrony ca-certificates openssl libcurl embiggen-disk dhcpcd wpa_supplicant alsa-lib alsa-utils pipewire wireplumber bluez5_utils sbc libsndfile lxc wana-waydroid

define WANA_PLATFORM_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0644 $(@D)/services/00-system-bus.service $(TARGET_DIR)/etc/wana/services.d/system-bus.service
	$(INSTALL) -D -m 0644 $(@D)/services/05-grow-root.service $(TARGET_DIR)/etc/wana/services.d/grow-root.service
	$(INSTALL) -D -m 0644 $(@D)/services/10-user-bus.service $(TARGET_DIR)/etc/wana/services.d/user-bus.service
	$(INSTALL) -D -m 0644 $(@D)/services/15-wpa-supplicant.service $(TARGET_DIR)/etc/wana/services.d/wpa-supplicant.service
	$(INSTALL) -D -m 0644 $(@D)/services/16-dhcpcd.service $(TARGET_DIR)/etc/wana/services.d/dhcpcd.service
	$(INSTALL) -D -m 0644 $(@D)/services/20-bluetooth.service $(TARGET_DIR)/etc/wana/services.d/bluetooth.service
	$(INSTALL) -D -m 0644 $(@D)/services/30-pipewire.service $(TARGET_DIR)/etc/wana/services.d/pipewire.service
	$(INSTALL) -D -m 0644 $(@D)/services/35-pipewire-pulse.service $(TARGET_DIR)/etc/wana/services.d/pipewire-pulse.service
	$(INSTALL) -D -m 0644 $(@D)/services/40-wireplumber.service $(TARGET_DIR)/etc/wana/services.d/wireplumber.service
	$(INSTALL) -D -m 0644 $(@D)/services/44-auth.service $(TARGET_DIR)/etc/wana/services.d/auth.service
	$(INSTALL) -D -m 0644 $(@D)/services/45-power.service $(TARGET_DIR)/etc/wana/services.d/power.service
	$(INSTALL) -D -m 0644 $(@D)/services/46-update.service $(TARGET_DIR)/etc/wana/services.d/update.service
	$(INSTALL) -D -m 0644 $(@D)/services/50-waydroid-container.service $(TARGET_DIR)/etc/wana/services.d/waydroid-container.service
	$(INSTALL) -D -m 0755 $(@D)/bin/wana-grow-root $(TARGET_DIR)/usr/bin/wana-grow-root
	$(INSTALL) -D -m 0755 $(@D)/bin/wana-audio $(TARGET_DIR)/usr/bin/wana-audio
	$(INSTALL) -D -m 0755 $(@D)/bin/wana-bluetooth $(TARGET_DIR)/usr/bin/wana-bluetooth
	$(INSTALL) -D -m 0755 $(@D)/bin/wana-wifi $(TARGET_DIR)/usr/bin/wana-wifi
	$(INSTALL) -D -m 0600 $(@D)/wifi.conf $(TARGET_DIR)/var/lib/wana/wifi.conf
	$(INSTALL) -D -m 0755 $(@D)/bin/wana-android $(TARGET_DIR)/usr/bin/wana-android
	$(INSTALL) -D -m 0755 $(@D)/bin/wana-waydroid-container $(TARGET_DIR)/usr/bin/wana-waydroid-container
	$(INSTALL) -D -m 0755 $(@D)/bin/wana-winrun $(TARGET_DIR)/usr/bin/wana-winrun
	$(INSTALL) -D -m 0644 $(@D)/android/lxc.conf $(TARGET_DIR)/etc/wana/android/lxc.conf
endef

$(eval $(generic-package))
