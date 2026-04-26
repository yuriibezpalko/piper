# SkyPulse drone firmware build container — reproducible OpenIPC + buildroot env.
#
# Build context = piper/drone-client/ (so main.cpp is reachable by drop/Makefile).
#
# One-shot build:
#   docker build -t piper-firmware -f drop/Dockerfile .
#   docker run --rm \
#       -v "$PWD/drop/output:/work/drop/output" \
#       -v "$PWD/drop/.openipc:/work/drop/.openipc" \
#       piper-firmware make BOARD=ssc338q_skypulse
#
# Or via the helper:
#   ./drop/build.sh ssc338q_skypulse

# OpenIPC's prebuilt cross-toolchain is x86_64 only — force amd64 so Docker
# Desktop's Rosetta emulation kicks in on Apple Silicon hosts.
FROM --platform=linux/amd64 debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 LC_ALL=C.UTF-8

# Buildroot deps + a few extras OpenIPC's tree pulls.
RUN apt-get update && apt-get install -y --no-install-recommends \
        automake autotools-dev bc build-essential cpio curl file git \
        libncurses-dev libtool lzop make rsync unzip wget libssl-dev \
        ca-certificates python3 bison flex gawk perl xxd \
        u-boot-tools device-tree-compiler whiptail sudo \
    && rm -rf /var/lib/apt/lists/*

# Buildroot refuses to run as root. Keep UID stable so bind-mounted
# output/ and .openipc/ are owned by the host user.
RUN useradd -m -u 1000 -s /bin/bash builder && \
    echo "builder ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/builder
WORKDIR /work

# Stage the whole drone-client/ — drop/Makefile copies main.cpp into the
# skypulse package at stage time using SKYPULSE_SRC (default = ../).
COPY --chown=builder:builder . /work/

USER builder
WORKDIR /work/drop

ENTRYPOINT ["make"]
CMD ["help"]
