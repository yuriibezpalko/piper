# Makefile — skypulse-drone for OpenIPC SSC30KQ
#
# Toolchain auto-detection order:
#   1. arm-openipc-linux-gnueabihf-g++  (OpenIPC official)
#   2. arm-linux-gnueabihf-g++          (Debian/Ubuntu: apt install g++-arm-linux-gnueabihf)
#   3. arm-unknown-linux-gnueabihf-g++  (macOS Homebrew messense tap)
#
# Override: make CXX=/full/path/to/cross-g++

HOMEBREW_PREFIX := $(shell brew --prefix 2>/dev/null || echo /opt/homebrew)
MACOS_CXX_PATH   = $(HOMEBREW_PREFIX)/opt/arm-unknown-linux-gnueabihf/bin/arm-unknown-linux-gnueabihf-g++

ifeq ($(origin CXX), default)
  ifneq ($(shell which arm-openipc-linux-gnueabihf-g++ 2>/dev/null),)
    CXX := arm-openipc-linux-gnueabihf-g++
  else ifneq ($(shell which arm-linux-gnueabihf-g++ 2>/dev/null),)
    CXX := arm-linux-gnueabihf-g++
  else ifneq ($(shell which arm-unknown-linux-gnueabihf-g++ 2>/dev/null),)
    CXX := arm-unknown-linux-gnueabihf-g++
  else ifeq ($(shell test -f $(MACOS_CXX_PATH) && echo yes),yes)
    CXX := $(MACOS_CXX_PATH)
  else
    $(error No ARM cross-compiler found. Run: brew install messense/macos-cross-toolchains/arm-unknown-linux-gnueabihf  OR  sudo apt install g++-arm-linux-gnueabihf)
  endif
endif

CXXFLAGS = -march=armv7-a -mfpu=neon -mfloat-abi=hard \
           -O2 -flto -fdata-sections -ffunction-sections \
           -mtune=cortex-a7
LDFLAGS  = -Wl,--gc-sections -pthread
TARGET   = skypulse-drone

.PHONY: all native clean deploy strip

all: $(TARGET)
	@echo "Built with: $(CXX)"

$(TARGET): main.cpp
	$(CXX) $(CXXFLAGS) -o $@ $< $(LDFLAGS)
	@file $@

native: main.cpp
	g++ -O2 -std=c++17 -pthread -o $(TARGET)-native $< || true

strip: $(TARGET)
	arm-unknown-linux-gnueabihf-strip $(TARGET) 2>/dev/null || strip $(TARGET)

clean:
	rm -f $(TARGET) $(TARGET)-native

deploy: $(TARGET)
	@test -n "$(IP)" || (echo "Usage: make deploy IP=<camera-ip>"; exit 1)
	scp -O $(TARGET)      root@$(IP):/usr/bin/$(TARGET)
	scp -O S95skypulse    root@$(IP):/etc/init.d/S95skypulse
	scp -O skypulse.conf  root@$(IP):/etc/skypulse.conf
	ssh root@$(IP) "chmod +x /usr/bin/$(TARGET) /etc/init.d/S95skypulse"
	@echo "Deployed to $(IP)"
