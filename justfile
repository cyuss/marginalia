# Marginalia — task runner
#
# `just` is the primary entry point; the Makefile mirrors it exactly for anyone
# who would rather not install another tool. Recipe names are identical in both,
# so instructions can say `just test` or `make test` interchangeably.
#
#   just            list everything
#   just check      everything CI runs
#   just device     what you can do to a connected reMarkable

set shell := ["bash", "-uc"]

RM_TARGET := "armv7-unknown-linux-gnueabihf"
PORTABLE  := "-p marginalia-core -p marginalia-safety -p marginalia-observability -p marginalia-remarkable -p marginalia-platform -p marginalia-zotero -p marginalia-library-folder -p marginalia-annotations"

# List every recipe, grouped.
default:
    @just --list --unsorted

# ─────────────────────────────────────────────────────────────────────────────
# Setup
# ─────────────────────────────────────────────────────────────────────────────

# Everything needed to build and test. Run once.
setup: setup-rust setup-node
    @echo ""
    @echo "Ready. Try: just check"

# Rust toolchain plus the reMarkable's ARM target.
setup-rust:
    rustup target add {{RM_TARGET}}
    @echo "Rust ready ($(rustc --version))"

# JavaScript dependencies for the desktop companion (needs Node 20+).
setup-node:
    # If this fails with URL.canParse, your Node is too old:
    #   nvm install 20 && nvm use 20 && corepack prepare pnpm@9 --activate
    pnpm install

# Install `cross`, which builds the agent for the device (needs Docker).
setup-cross:
    cargo install cross

# ─────────────────────────────────────────────────────────────────────────────
# Develop
# ─────────────────────────────────────────────────────────────────────────────

# Run the desktop companion (Tauri window).
dev:
    pnpm dev

# Run only the interface, in a browser at http://localhost:1420.
dev-web:
    pnpm dev:web

# Run the on-device agent locally — `just agent doctor`.
agent *ARGS="status":
    # Not $TMPDIR: on macOS that lives under /var, which the agent refuses to
    # write to. The refusal is correct -- /var belongs to the device -- so the
    # dev home goes somewhere unambiguously ours instead.
    MARGINALIA_HOME="${HOME}/.marginalia-dev" cargo run -q -p marginalia-agent -- {{ARGS}}

# Create the agent's local scratch home and database.
agent-init:
    @just agent init

# ─────────────────────────────────────────────────────────────────────────────
# Verify
# ─────────────────────────────────────────────────────────────────────────────

# Everything CI runs. Run this before opening a pull request.
check: fmt-check lint test test-safety cross-check
    @echo ""
    @echo "All checks passed."

# The whole Rust test suite.
test:
    cargo test --workspace --all-features

# The mandatory safety suite, with output. A failure here is never acceptable.
test-safety:
    cargo test -p marginalia-safety-suite --all-features -- --nocapture

# Dependency-direction and forbidden-import rules.
test-arch:
    cargo test -p marginalia-architecture-tests

# Phase 0 behaviour, pinned before any of it moves.
test-characterization:
    cargo test -p marginalia-characterization-tests

# Device-side faults: power loss, corruption, storage pressure, clock skew.
test-device-faults:
    cargo test -p marginalia-simulator --test device_faults -- --nocapture

# Talk to the real Zotero API (needs MARGINALIA_ZOTERO_API_KEY; skips without it).
test-zotero-live:
    cargo test -p marginalia-zotero --features http -- --ignored --nocapture

# Clippy, warnings denied.
lint:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

# Format everything.
fmt:
    cargo fmt --all

# Fail if anything is unformatted.
fmt-check:
    cargo fmt --all -- --check

# TypeScript, strict.
typecheck:
    pnpm typecheck

# ─────────────────────────────────────────────────────────────────────────────
# Build
# ─────────────────────────────────────────────────────────────────────────────

# Release build of everything that builds on this machine.
build:
    cargo build --release --workspace

# Prove the portable crates still compile for the reMarkable.
cross-check:
    # Excludes marginalia-database, whose SQLite is compiled from C and needs a
    # cross toolchain (U17). Use `build-device` for the real thing.
    cargo check --target {{RM_TARGET}} {{PORTABLE}}

# Build the agent for the reMarkable. Needs `cross` (or a host cross-gcc).
build-device:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v cross >/dev/null 2>&1; then
        cross build --release --target {{RM_TARGET}} -p marginalia-agent
    elif command -v arm-linux-gnueabihf-gcc >/dev/null 2>&1; then
        cargo build --release --target {{RM_TARGET}} -p marginalia-agent
    else
        echo "No ARM cross-compiler. Run: just setup-cross" >&2
        exit 1
    fi
    ls -lh target/{{RM_TARGET}}/release/marginalia

# Build the agent for the reMarkable inside a container. Needs only Docker.
build-device-docker:
    ./tools/device/build-in-docker.sh

# Run the built ARM agent under emulation, to check it actually executes.
verify-device-binary:
    #!/usr/bin/env bash
    set -euo pipefail
    BIN=target/device/{{RM_TARGET}}/release/marginalia
    [[ -f "$BIN" ]] || { echo "Build it first: just build-device-docker" >&2; exit 1; }
    file "$BIN"
    docker run --rm --platform linux/arm/v7 \
      -v "$PWD/target/device/{{RM_TARGET}}/release":/x:ro \
      -e MARGINALIA_HOME=/data/.marginalia \
      debian:bookworm-slim bash -c '/x/marginalia init && /x/marginalia doctor'

# Build the desktop companion for distribution.
build-desktop:
    pnpm build

# ─────────────────────────────────────────────────────────────────────────────
# Device — everything below talks to a connected reMarkable
# ─────────────────────────────────────────────────────────────────────────────

# What you can do to a connected reMarkable.
device:
    @echo "  just device-doctor       check everything, change nothing"
    @echo "  just device-install-dry  show what installing would do"
    @echo "  just device-install      install"
    @echo "  just device-status       ask the installed agent how it is"
    @echo "  just device-reset-dry    show what removing would take"
    @echo "  just device-reset        remove it, and verify it is gone"
    @echo ""
    @echo "  RM_HOST=$(echo ${RM_HOST:-10.11.99.1})  (set it to use Wi-Fi)"

# Read-only checks against your machine and your reMarkable.
device-doctor:
    ./tools/device/doctor.sh

# Show every step of an install without performing any of it.
device-install-dry:
    ./tools/device/install.sh --dry-run

# Install the agent on the connected reMarkable.
device-install:
    ./tools/device/install.sh

# Ask the installed agent to report its state.
device-status:
    ssh "root@${RM_HOST:-10.11.99.1}" '/home/root/.marginalia/bin/marginalia status'

# Ask the installed agent to check itself.
device-check:
    ssh "root@${RM_HOST:-10.11.99.1}" '/home/root/.marginalia/bin/marginalia doctor'

# List exactly what removal would take, without taking it.
device-reset-dry:
    ./tools/device/reset.sh --dry-run

# Remove Marginalia and return the device to stock.
device-reset:
    ./tools/device/reset.sh

# ─────────────────────────────────────────────────────────────────────────────
# Housekeeping
# ─────────────────────────────────────────────────────────────────────────────

# Remove build artefacts.
clean:
    cargo clean
    rm -rf apps/desktop/dist apps/desktop/node_modules node_modules

# The documents worth reading first.
docs:
    @echo "  README.md                                    what this is"
    @echo "  docs/INSTALL.md                              install on your computer"
    @echo "  docs/INSTALL_REMARKABLE.md                   install on your reMarkable"
    @echo "  docs/USING_MARGINALIA.md                     how to actually use it"
    @echo "  ROADMAP.md                                   what is built and what is next"
    @echo ""
    @echo "  docs/architecture/ARCHITECTURE.md            the design"
    @echo "  docs/safety/SAFETY_MODEL.md                  what protects your device"
    @echo "  docs/safety/DEVICE_WRITE_POLICY.md           exactly what may be written"
    @echo "  docs/adr/                                    decisions, including the open ones"
    @echo "  docs/development/OPEN_QUESTIONS.md           what is still unknown"
    @echo "  docs/remarkable/HARDWARE_VALIDATION.md       what a real device did"

# Count what exists, for a sense of scale.
stats:
    @echo "Rust:  $(find packages apps tests -name '*.rs' | xargs wc -l | tail -1 | awk '{print $1}') lines"
    @echo "Tests: $(grep -rc '#\[test\]' --include='*.rs' packages apps tests | awk -F: '{s+=$2} END {print s}')"
    @echo "Docs:  $(find docs -name '*.md' | wc -l | tr -d ' ') documents"
