################################################################################
#
# wana-fonts
#
################################################################################

# google/fonts commit the files are taken from (the same one Buildroot's
# googlefontdirectory pins). Only the three files are downloaded, not the
# repository archive; wana-fonts.hash pins their contents.
WANA_FONTS_VERSION = 2125bc9b447971543caaa132530b828e5e682819
WANA_FONTS_SOURCE =
WANA_FONTS_LICENSE = OFL-1.1
WANA_FONTS_LICENSE_FILES = OFL-arabic.txt OFL-latin.txt

WANA_FONTS_URL = https://raw.githubusercontent.com/google/fonts/$(WANA_FONTS_VERSION)/ofl
# Upstream file names contain [ ] and , (variable-font axes); they are
# URL-encoded here, which is also the name they get in the download dir.
# Installed under plain names (-VF: variable font).
WANA_FONTS_EXTRA_DOWNLOADS = \
	$(WANA_FONTS_URL)/notonaskharabic/NotoNaskhArabic%5Bwght%5D.ttf \
	$(WANA_FONTS_URL)/notosansarabic/NotoSansArabic%5Bwdth%2Cwght%5D.ttf \
	$(WANA_FONTS_URL)/notosans/NotoSans%5Bwdth%2Cwght%5D.ttf

define WANA_FONTS_EXTRACT_CMDS
	cp $(WANA_FONTS_PKGDIR)/OFL-arabic.txt $(WANA_FONTS_PKGDIR)/OFL-latin.txt $(@D)/
endef

WANA_FONTS_TARGET_DIR = $(TARGET_DIR)/usr/share/fonts/wana

define WANA_FONTS_INSTALL_TARGET_CMDS
	$(INSTALL) -D -m 0644 $(WANA_FONTS_DL_DIR)/NotoNaskhArabic%5Bwght%5D.ttf \
		$(WANA_FONTS_TARGET_DIR)/NotoNaskhArabic-VF.ttf
	$(INSTALL) -D -m 0644 $(WANA_FONTS_DL_DIR)/NotoSansArabic%5Bwdth%2Cwght%5D.ttf \
		$(WANA_FONTS_TARGET_DIR)/NotoSansArabic-VF.ttf
	$(INSTALL) -D -m 0644 $(WANA_FONTS_DL_DIR)/NotoSans%5Bwdth%2Cwght%5D.ttf \
		$(WANA_FONTS_TARGET_DIR)/NotoSans-VF.ttf
	cd $(WANA_FONTS_TARGET_DIR) && \
		sha256sum NotoNaskhArabic-VF.ttf NotoSansArabic-VF.ttf NotoSans-VF.ttf > SHA256SUMS
	$(INSTALL) -D -m 0644 $(@D)/OFL-arabic.txt $(TARGET_DIR)/usr/share/licenses/wana-fonts/OFL-arabic.txt
	$(INSTALL) -D -m 0644 $(@D)/OFL-latin.txt $(TARGET_DIR)/usr/share/licenses/wana-fonts/OFL-latin.txt
endef

$(eval $(generic-package))
