#!/usr/bin/env bash
# Convenience wrapper — builds the firmware in a container.
#
#   ./build.sh                     # interactive board picker
#   ./build.sh ssc338q_skypulse    # one-shot build
#   ./build.sh ssc30kq_skypulse
#   ./build.sh --clean             # drop all cached docker volumes
#
# Output (final firmware archive) lands in drop/output/images/.
#
# Why named volumes? Docker Desktop on macOS bind-mounts don't support
# rsync --hard-links, which buildroot's per-package mode needs. Build state
# lives on Linux-native named volumes; only the final artefacts get copied
# back to the host bind-mount at the end.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
DRONE_CLIENT_DIR="$(dirname "$HERE")"
IMAGE="piper-firmware:latest"
VOL_BUILD="piper-firmware-build"
VOL_OPENIPC="piper-firmware-openipc"

if [[ "${1:-}" == "--clean" ]]; then
    echo "==> removing docker volumes"
    docker volume rm -f "$VOL_BUILD" "$VOL_OPENIPC" 2>/dev/null || true
    echo "==> removing image"
    docker rmi -f "$IMAGE" 2>/dev/null || true
    exit 0
fi

BOARD="${1:-}"

# OpenIPC cross-toolchain is x86_64; force amd64 platform on Apple Silicon.
export DOCKER_DEFAULT_PLATFORM=linux/amd64

echo "==> building docker image $IMAGE (linux/amd64)"
docker build \
    --platform linux/amd64 \
    --file "$HERE/Dockerfile" \
    --tag "$IMAGE" \
    "$DRONE_CLIENT_DIR"

echo "==> ensuring named volumes exist (build state, openipc clone)"
docker volume create "$VOL_BUILD"   >/dev/null
docker volume create "$VOL_OPENIPC" >/dev/null

echo "==> fixing volume ownership (uid 1000 = builder)"
docker run --rm \
    -v "$VOL_BUILD:/work/drop/output" \
    -v "$VOL_OPENIPC:/work/drop/.openipc" \
    --user root \
    --entrypoint chown \
    "$IMAGE" \
    -R 1000:1000 /work/drop/output /work/drop/.openipc

mkdir -p "$HERE/output"

TTY_FLAGS=()
[[ -t 0 ]] && TTY_FLAGS=(-it)

echo "==> running build (BOARD=${BOARD:-<picker>}, SAFE_MODE=1, JOBS=1)"
docker run --rm "${TTY_FLAGS[@]}" \
    --ulimit nofile=65536:65536 \
    -v "$VOL_BUILD:/work/drop/output" \
    -v "$VOL_OPENIPC:/work/drop/.openipc" \
    "$IMAGE" \
    SAFE_MODE=1 JOBS=1 ${BOARD:+BOARD=$BOARD}

echo "==> extracting firmware artefacts to $HERE/output/images/"
docker run --rm \
    -v "$VOL_BUILD:/src:ro" \
    -v "$HERE/output:/dst" \
    --entrypoint sh \
    "$IMAGE" \
    -c 'mkdir -p /dst/images && cp -r /src/images/. /dst/images/'

echo
echo "==> firmware artefacts:"
ls -lh "$HERE/output/images/"*.tgz 2>/dev/null || echo "  (no tgz produced — build failed?)"
