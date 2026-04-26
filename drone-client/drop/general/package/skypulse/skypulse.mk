################################################################################
#
# skypulse-drone — CRSF/UART bridge for OpenIPC drone client
#
# Source (main.cpp) is copied into package/skypulse/files/ by the top-level
# drop/Makefile prepare step from piper/drone-client/main.cpp. The cross-compile
# uses buildroot's TARGET_CXX so it picks up the OpenIPC SigmaStar toolchain.
#
################################################################################

SKYPULSE_VERSION = 1.0.0
SKYPULSE_SITE = $(BR2_EXTERNAL_GENERAL_PATH)/package/skypulse/files
SKYPULSE_SITE_METHOD = local
SKYPULSE_LICENSE = MIT

define SKYPULSE_BUILD_CMDS
	$(TARGET_CXX) -march=armv7-a -mfpu=neon -mfloat-abi=hard \
		-O2 -flto -fdata-sections -ffunction-sections -mtune=cortex-a7 \
		-pthread -Wl,--gc-sections \
		-o $(@D)/skypulse-drone $(@D)/main.cpp
endef

define SKYPULSE_INSTALL_TARGET_CMDS
	$(INSTALL) -m 0755 -D $(@D)/skypulse-drone $(TARGET_DIR)/usr/bin/skypulse-drone
	$(INSTALL) -m 0644 -D $(SKYPULSE_PKGDIR)/files/skypulse.conf $(TARGET_DIR)/etc/skypulse.conf
	$(INSTALL) -m 0755 -D $(SKYPULSE_PKGDIR)/files/S97skypulse  $(TARGET_DIR)/etc/init.d/S97skypulse
endef

$(eval $(generic-package))
