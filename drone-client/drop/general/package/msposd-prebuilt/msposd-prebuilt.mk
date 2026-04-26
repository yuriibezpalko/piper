################################################################################
#
# msposd (prebuilt) — OpenIPC OSD daemon
#
# Pulls the binary + fonts from the upstream "latest" release tag — see
# https://github.com/OpenIPC/msposd#install
#
################################################################################

MSPOSD_PREBUILT_VERSION  = latest
MSPOSD_PREBUILT_SITE     = https://github.com/OpenIPC/msposd/releases/download/$(MSPOSD_PREBUILT_VERSION)
MSPOSD_PREBUILT_SOURCE   = msposd_star6e
MSPOSD_PREBUILT_LICENSE  = GPL-2.0

# Don't try to extract — single ELF binary, not an archive.
define MSPOSD_PREBUILT_EXTRACT_CMDS
	cp $(MSPOSD_PREBUILT_DL_DIR)/$(MSPOSD_PREBUILT_SOURCE) $(@D)/msposd
endef

# Pull fonts as extra downloads.
MSPOSD_PREBUILT_FONTS_URL = https://raw.githubusercontent.com/openipc/msposd/main/fonts
MSPOSD_PREBUILT_EXTRA_DOWNLOADS = \
	$(MSPOSD_PREBUILT_FONTS_URL)/font_inav.png \
	$(MSPOSD_PREBUILT_FONTS_URL)/font_inav_hd.png

define MSPOSD_PREBUILT_INSTALL_TARGET_CMDS
	$(INSTALL) -m 0755 -D $(@D)/msposd $(TARGET_DIR)/usr/bin/msposd
	$(INSTALL) -m 0644 -D $(MSPOSD_PREBUILT_DL_DIR)/font_inav.png    $(TARGET_DIR)/usr/share/fonts/font.png
	$(INSTALL) -m 0644 -D $(MSPOSD_PREBUILT_DL_DIR)/font_inav_hd.png $(TARGET_DIR)/usr/share/fonts/font_hd.png
	$(INSTALL) -m 0755 -D $(MSPOSD_PREBUILT_PKGDIR)/files/S96msposd  $(TARGET_DIR)/etc/init.d/S96msposd
	$(INSTALL) -m 0644 -D $(MSPOSD_PREBUILT_PKGDIR)/files/msposd.conf $(TARGET_DIR)/etc/msposd.conf
endef

$(eval $(generic-package))
