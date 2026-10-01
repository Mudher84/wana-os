################################################################################
#
# wana-wine64
#
################################################################################

WANA_WINE64_VERSION = 11.0
WANA_WINE64_SOURCE = wine-$(WANA_WINE64_VERSION).tar.xz
WANA_WINE64_SITE = https://dl.winehq.org/wine/source/11.0
WANA_WINE64_LICENSE = LGPL-2.1+
WANA_WINE64_LICENSE_FILES = COPYING.LIB LICENSE
WANA_WINE64_DEPENDENCIES = host-bison host-flex host-wana-wine64 \
	wayland alsa-lib dbus fontconfig freetype pulseaudio udev
HOST_WANA_WINE64_DEPENDENCIES = host-bison host-flex host-freetype

WANA_WINE64_CONF_OPTS = \
	--with-wine-tools=../host-wana-wine64-$(WANA_WINE64_VERSION) \
	--disable-tests \
	--enable-win64 \
	--enable-tools \
	--without-capi \
	--without-coreaudio \
	--without-cups \
	--with-dbus \
	--without-ffmpeg \
	--with-fontconfig \
	--with-freetype \
	--without-gettext \
	--without-gettextpo \
	--without-gphoto \
	--without-gnutls \
	--without-gssapi \
	--without-gstreamer \
	--without-krb5 \
	--without-mingw \
	--without-netapi \
	--without-opencl \
	--without-opengl \
	--without-oss \
	--without-pcap \
	--without-pcsclite \
	--with-pulse \
	--without-sane \
	--without-sdl \
	--with-udev \
	--without-usb \
	--without-v4l2 \
	--without-vulkan \
	--with-wayland \
	--without-x \
	--without-xcomposite \
	--without-xcursor \
	--without-xfixes \
	--without-xinerama \
	--without-xinput \
	--without-xinput2 \
	--without-xrandr \
	--without-xrender \
	--without-xshape \
	--without-xshm \
	--without-xxf86vm

HOST_WANA_WINE64_CONF_OPTS = \
	--enable-win64 \
	--disable-tests \
	--disable-win16 \
	--without-alsa \
	--without-capi \
	--without-coreaudio \
	--without-cups \
	--without-dbus \
	--without-ffmpeg \
	--with-freetype \
	--without-gettext \
	--without-gettextpo \
	--without-gphoto \
	--without-gnutls \
	--without-gssapi \
	--without-gstreamer \
	--without-krb5 \
	--without-mingw \
	--without-netapi \
	--without-opencl \
	--without-opengl \
	--without-osmesa \
	--without-oss \
	--without-pcap \
	--without-pcsclite \
	--without-pulse \
	--without-sane \
	--without-sdl \
	--without-udev \
	--without-usb \
	--without-v4l2 \
	--without-vulkan \
	--without-wayland \
	--without-x \
	--without-xcomposite \
	--without-xcursor \
	--without-xfixes \
	--without-xinerama \
	--without-xinput \
	--without-xinput2 \
	--without-xrandr \
	--without-xrender \
	--without-xshape \
	--without-xshm \
	--without-xxf86vm

define HOST_WANA_WINE64_BUILD_CMDS
	$(HOST_MAKE_ENV) $(MAKE) -C $(@D) __tooldeps__
endef

define HOST_WANA_WINE64_INSTALL_CMDS
	:
endef

$(eval $(autotools-package))
$(eval $(host-autotools-package))
