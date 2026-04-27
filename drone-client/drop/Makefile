# SkyPulse drone firmware — top-level orchestrator.
#
# Stages our drop tree onto a clone of OpenIPC/firmware and delegates the build
# to upstream's Makefile (same approach as quadrofleet-masina).
#
# Usage:
#   make BOARD=ssc338q_skypulse           # full build
#   make BOARD=ssc30kq_skypulse
#   make stage                            # just stage files, don't build
#   make clean | distclean
#   make list                             # available board configs

OPENIPC      ?= $(CURDIR)/.openipc
OPENIPC_REPO  = https://github.com/OpenIPC/firmware.git

# Buildroot output path — short by default to dodge libffi's deep-path bug
# (CI overrides to /b/output; locally it lives next to the makefile).
TARGET       ?= $(CURDIR)/output

# main.cpp lives one level up — drone-client/main.cpp.
SKYPULSE_SRC  ?= $(realpath $(CURDIR)/..)
SKYPULSE_FILES = $(OPENIPC)/general/package/skypulse/files

PKG_CONFIG_IN  = $(OPENIPC)/general/package/Config.in
PIPER_PKGS     = skypulse msposd-prebuilt

# Parallelism — upstream hardcodes -j$(nproc); we patch it to -j$(JOBS).
# Defaults to nproc on real Linux. On Mac/Rosetta build.sh sets JOBS=1.
JOBS ?= $(shell nproc 2>/dev/null || echo 4)

# Conservative-mode toggle for Apple Silicon Rosetta (clears jobserver state,
# disables per-package directories). build.sh sets this to 1; on real Linux
# leave it at 0 for full parallel buildroot performance.
SAFE_MODE ?= 0

.PHONY: all stage prepare clean distclean list deps help

ifeq ($(SAFE_MODE),1)
INVOKE = env -i HOME=$$HOME PATH=$$PATH PWD=$(OPENIPC) \
	MAKEFLAGS= MFLAGS= MAKELEVEL= MAKE_TERMOUT=1 MAKE_TERMERR=1
else
INVOKE = PWD=$(OPENIPC)
endif

all: stage
	@cd $(OPENIPC) && $(INVOKE) $(MAKE) TARGET=$(TARGET) \
		$(if $(BOARD),BOARD=$(BOARD)) br-toolchain
	@cd $(OPENIPC) && $(INVOKE) $(MAKE) TARGET=$(TARGET) \
		$(if $(BOARD),BOARD=$(BOARD))

# Stage: clone upstream, drop our files on top, register Config.in entries.
stage: prepare
	@echo "--- merging piper drop into $(OPENIPC) ---"
	@cp -rf $(CURDIR)/general/. $(OPENIPC)/general/
	@cp -rf $(CURDIR)/br-ext-chip-sigmastar/. $(OPENIPC)/br-ext-chip-sigmastar/
	@echo "--- staging skypulse main.cpp ($(SKYPULSE_SRC)/main.cpp) ---"
	@test -f $(SKYPULSE_SRC)/main.cpp || (echo "ERROR: $(SKYPULSE_SRC)/main.cpp not found"; exit 1)
	@mkdir -p $(SKYPULSE_FILES)
	@cp -f $(SKYPULSE_SRC)/main.cpp $(SKYPULSE_FILES)/main.cpp
	@echo "--- registering piper packages in general/package/Config.in ---"
	@for pkg in $(PIPER_PKGS); do \
		grep -q "package/$$pkg/Config.in" $(PKG_CONFIG_IN) || \
		echo "source \"\$$BR2_EXTERNAL_GENERAL_PATH/package/$$pkg/Config.in\"" >> $(PKG_CONFIG_IN); \
	done
	@echo "--- patching upstream Makefile: -j\$$(shell nproc) -> -j$(JOBS) ---"
	@sed -i 's|-j\$$(shell nproc)|-j$(JOBS)|g' $(OPENIPC)/Makefile
ifeq ($(SAFE_MODE),1)
	@echo "--- SAFE_MODE: disabling per-package mode in openipc.fragment ---"
	@sed -i 's/^BR2_PER_PACKAGE_DIRECTORIES=y/BR2_PER_PACKAGE_DIRECTORIES=n/' \
		$(OPENIPC)/general/openipc.fragment
endif

prepare:
	@if test ! -f $(OPENIPC)/Makefile; then \
		echo "--- cloning OpenIPC firmware base into $(OPENIPC) ---"; \
		mkdir -p $(OPENIPC); \
		git clone --depth 1 $(OPENIPC_REPO) /tmp/openipc-clone; \
		cp -a /tmp/openipc-clone/. $(OPENIPC)/; \
		rm -rf /tmp/openipc-clone; \
	fi

list: prepare
	@find $(OPENIPC)/br-ext-*/configs $(CURDIR)/br-ext-*/configs -name '*_defconfig' 2>/dev/null \
		| sed 's|.*/||;s|_defconfig$$||' | sort -u

clean:
	@$(MAKE) -C $(OPENIPC) clean 2>/dev/null || true

distclean:
	@rm -rf $(OPENIPC)

deps:
	sudo apt-get install -y automake autotools-dev bc build-essential cpio \
		curl file fzf git libncurses-dev libtool lzop make rsync unzip wget libssl-dev

help:
	@echo "Targets:"
	@echo "  make BOARD=<name>   build firmware (e.g. ssc338q_skypulse)"
	@echo "  make stage          stage files only"
	@echo "  make list           list available boards"
	@echo "  make clean          clean buildroot artefacts (keeps clone)"
	@echo "  make distclean      remove .openipc/ entirely"
