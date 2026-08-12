#!/usr/bin/env bash
# Build the agent for the reMarkable inside a container.
#
#   ./tools/device/build-in-docker.sh              with networking (Zotero)
#   ./tools/device/build-in-docker.sh --no-network smaller, offline-only build
#
# ── Why this exists alongside `cross` ────────────────────────────────────────
#
# `cross` is the usual answer, and it shells out to `rustup` on the host. A
# machine can have a working Rust toolchain without a `rustup` binary — cargo
# and rustc shims are enough for everything else — and then `cross` fails with
# a confusing "could not execute rustup toolchain list".
#
# This does the same job with one `docker run`: it needs Docker and nothing
# else from the host, and it leaves the host's Rust installation alone.

source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET="armv7-unknown-linux-gnueabihf"

# Matches rust-toolchain.toml. A mismatch would build against a different
# compiler than everything else in the project.
RUST_IMAGE="rust:1.90-bookworm"

FEATURES="--features network"
LABEL="with Zotero networking"
if [[ "${1:-}" == "--no-network" ]]; then
  FEATURES=""
  LABEL="offline only"
fi

# Docker Desktop does not always put its CLI on the PATH.
if ! command -v docker >/dev/null 2>&1; then
  for candidate in /Applications/Docker.app/Contents/Resources/bin \
                   /usr/local/bin /opt/homebrew/bin; do
    if [[ -x "$candidate/docker" ]]; then
      PATH="$candidate:$PATH"
      break
    fi
  done
fi

command -v docker >/dev/null 2>&1 \
  || die "docker not found" "Install Docker, or build with a host cross-gcc instead."

docker info >/dev/null 2>&1 \
  || die "the Docker daemon is not running" \
         "Start Docker Desktop, or run: colima start"

say "${C_BOLD}Building the agent for your reMarkable${C_OFF}"
info "target   ${TARGET}"
info "image    ${RUST_IMAGE}"
info "features ${LABEL}"

# The output goes to a separate directory so a container build (root-owned,
# Linux artefacts) never collides with the host's own target/ tree.
OUT_DIR="target/device"

# Build artefacts live in a Docker volume, not in the bind-mounted repository.
#
# WHY: rustc memory-maps the .rmeta files it has just written. Across macOS's
# bind mount that mapping is not reliable -- a second build against a populated
# target directory died with `SIGBUS: access to undefined memory` inside rustc,
# reproducibly, and the only cure was deleting the whole directory. Keeping the
# target tree on a Linux-native volume removes the mapping from the equation,
# and has the side benefit that the cache now survives between builds instead of
# being thrown away to work around the crash.
#
# Only the finished binary crosses back, as a plain copy of one file.
CACHE_VOLUME="marginalia-armv7-target"

step "Compiling"
docker run --rm \
  -v "$REPO_ROOT":/work \
  -v "${CACHE_VOLUME}:/target" \
  -w /work \
  -e CARGO_TARGET_DIR=/target \
  "$RUST_IMAGE" \
  bash -euc "
    apt-get update -qq
    apt-get install -y -qq gcc-arm-linux-gnueabihf >/dev/null
    rustup target add ${TARGET} >/dev/null

    export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_GNUEABIHF_LINKER=arm-linux-gnueabihf-gcc
    export CC_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-gcc
    export AR_armv7_unknown_linux_gnueabihf=arm-linux-gnueabihf-ar

    cargo build --release --target ${TARGET} -p marginalia-agent ${FEATURES}

    # One file back across the bind mount, owned by the user rather than root.
    mkdir -p /work/${OUT_DIR}/${TARGET}/release
    cp /target/${TARGET}/release/marginalia /work/${OUT_DIR}/${TARGET}/release/marginalia
    chown -R $(id -u):$(id -g) /work/${OUT_DIR}
  " || die "the build failed" "Nothing was sent to your reMarkable."

BINARY="${REPO_ROOT}/${OUT_DIR}/${TARGET}/release/marginalia"
[[ -f "$BINARY" ]] || die "the build produced no binary at ${BINARY}"

ok "built $(du -h "$BINARY" | awk '{print $1}')"
info "$(file -b "$BINARY" 2>/dev/null | cut -c1-90)"
say ""
say "  ${BINARY}"
say ""
say "  Install it:  ./tools/device/install.sh"
say ""
