#!/usr/bin/env bash
# Install Marginalia on a reMarkable connected to this machine.
#
# What it does, in full:
#   1. checks it can reach the device
#   2. builds the agent for the device's processor
#   3. copies it into ONE directory that Marginalia owns
#   4. writes a manifest of every file it placed, with checksums
#   5. initialises the agent's database
#
# What it never does: touch the device's own software. Not xochitl, not the
# kernel, not a system directory, not a startup script, not your documents.
#
#   ./tools/device/install.sh              install or update
#   ./tools/device/install.sh --dry-run    show every step, change nothing

source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

DRY_RUN=0
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=1

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET="armv7-unknown-linux-gnueabihf"
BINARY="marginalia"

say "${C_BOLD}Installing Marginalia on your reMarkable${C_OFF}"
(( DRY_RUN )) && say "${C_WARN}Dry run — nothing will be changed.${C_OFF}"

assert_home_is_safe

# ── 1. the device ────────────────────────────────────────────────────────────
step "1 · Finding your reMarkable"
require_device
ok "connected at ${RM_HOST}"
firmware="$(device_description)"
info "firmware ${firmware}"

warn "This firmware has not been validated with Marginalia."
info "The agent will run read-only until it has been. It cannot modify"
info "your device's software in any case — see docs/safety/DEVICE_WRITE_POLICY.md"

# ── 2. build ─────────────────────────────────────────────────────────────────
step "2 · Building the agent for your device"

# `cross` is the usual answer but it shells out to `rustup`, which a machine can
# lack while still having a perfectly good cargo. The container path needs only
# Docker, so it is tried whenever `cross` is unavailable or broken.
if command -v arm-linux-gnueabihf-gcc >/dev/null 2>&1; then
  builder="host"
elif command -v cross >/dev/null 2>&1 && command -v rustup >/dev/null 2>&1; then
  builder="cross"
else
  builder="docker"
fi
info "using ${builder}"

case "$builder" in
  host)   artifact="$REPO_ROOT/target/$TARGET/release/$BINARY" ;;
  cross)  artifact="$REPO_ROOT/target/$TARGET/release/$BINARY" ;;
  docker) artifact="$REPO_ROOT/target/device/$TARGET/release/$BINARY" ;;
esac

if (( DRY_RUN )); then
  info "would build with: ${builder}"
  info "would produce:    ${artifact}"
  artifact="(not built)"
else
  # Being installed is not the same as working. On a machine with both `cross`
  # and `rustup`, `cross` still fails if it cannot provision the toolchain
  # rust-toolchain.toml pins for the container's own architecture -- observed on
  # an arm64 Mac, where it tried to add a x86_64-unknown-linux-gnu toolchain and
  # gave up. So a failure of the preferred builder falls back to the container,
  # which needs only Docker. A real compile error fails both and still stops the
  # install; the cost of that is one wasted build, not a bad binary on a device.
  build_failed=0
  case "$builder" in
    host)
      ( cd "$REPO_ROOT" && cargo build --release --target "$TARGET" -p marginalia-agent --features network ) \
        || build_failed=1 ;;
    cross)
      ( cd "$REPO_ROOT" && cross build --release --target "$TARGET" -p marginalia-agent --features network ) \
        || build_failed=1 ;;
    docker)
      "$REPO_ROOT/tools/device/build-in-docker.sh" \
        || die "the build failed" "Nothing was sent to your reMarkable." ;;
  esac

  if (( build_failed )) && [[ "$builder" != "docker" ]]; then
    warn "${builder} could not build — falling back to the container"
    # No `command -v docker` guard here: Docker Desktop keeps its CLI off the
    # PATH, so that test reports "no Docker" on a machine that has it. The
    # container script already resolves the CLI and explains itself if it is
    # genuinely missing. One place that knows how to find Docker, not two.
    "$REPO_ROOT/tools/device/build-in-docker.sh" \
      || die "the build failed" "Nothing was sent to your reMarkable."
    builder="docker"
    artifact="$REPO_ROOT/target/device/$TARGET/release/$BINARY"
  fi

  [[ -f "$artifact" ]] || die "the build produced no binary at $artifact"
  ok "built $(du -h "$artifact" | awk '{print $1}')"
fi

# ── 3. storage check ─────────────────────────────────────────────────────────
step "3 · Checking there is room"
free_kb=$(rm_ssh "df -k /home | tail -n 1 | awk '{print \$4}'" | tr -d '\r')
info "$(( free_kb / 1024 )) MB free"
if (( free_kb < 51200 )); then
  die "less than 50 MB free on your reMarkable" \
      "Marginalia will not fill your device. Free some space and try again."
fi
ok "enough room, with the reserve intact"

# ── 4. install ───────────────────────────────────────────────────────────────
step "4 · Copying into ${MARGINALIA_HOME}"

if (( DRY_RUN )); then
  info "would create ${MARGINALIA_HOME}/bin"
  info "would copy the agent to ${MARGINALIA_HOME}/bin/${BINARY}"
  info "would write ${MARGINALIA_HOME}/${MANIFEST_NAME}"
  info "would run: ${MARGINALIA_HOME}/bin/${BINARY} init"
  say ""
  say "${C_BOLD}Dry run complete.${C_OFF} Nothing was changed."
  exit 0
fi

rm_ssh "mkdir -p '$MARGINALIA_HOME/bin' && chmod 700 '$MARGINALIA_HOME'"
ok "created ${MARGINALIA_HOME}"

rm_scp "$artifact" "${RM_USER}@${RM_HOST}:${MARGINALIA_HOME}/bin/${BINARY}"
rm_ssh "chmod 700 '$MARGINALIA_HOME/bin/${BINARY}'"
ok "copied the agent"

# Verify what arrived is what we sent. A truncated copy that runs is worse
# than one that does not.
local_sum=$(shasum -a 256 "$artifact" | awk '{print $1}')
remote_sum=$(rm_ssh "sha256sum '$MARGINALIA_HOME/bin/${BINARY}' 2>/dev/null | awk '{print \$1}'" | tr -d '\r')

if [[ -z "$remote_sum" ]]; then
  warn "the device has no sha256sum; skipping verification"
elif [[ "$local_sum" != "$remote_sum" ]]; then
  rm_ssh "rm -f '$MARGINALIA_HOME/bin/${BINARY}'"
  die "the copy did not arrive intact" \
      "It has been removed. Nothing else on your device was changed."
else
  ok "verified by checksum"
fi

# ── 5. manifest ──────────────────────────────────────────────────────────────
# Every file we placed, so that removal is exact rather than approximate.
step "5 · Recording what was installed"
# Read the version here, not inside the ssh command. Nested quoting sent the
# whole substitution to the device, where it ran against a BusyBox `cut` and a
# Cargo.toml that does not exist -- printing "cut: bad delimiter" and recording
# an empty version. A manifest that cannot say what it installed is not a
# manifest. Whatever is computed from this machine's checkout is computed on
# this machine.
version=$(awk -F'"' '/^version/ {print $2; exit}' "$REPO_ROOT/Cargo.toml")
[[ -n "$version" ]] || die "could not read the version from Cargo.toml" \
                          "The agent is installed but unrecorded; run reset.sh."

rm_ssh "cd '$MARGINALIA_HOME' && {
  printf '# Marginalia install manifest\n'
  printf '# version\t%s\n' '$version'
  printf '# installed\t%s\n' \"\$(date -u +%Y-%m-%dT%H:%M:%SZ)\"
  printf '# firmware\t%s\n' '$firmware'
  printf '%s\t%s\n' 'bin/${BINARY}' '$local_sum'
} > '$MANIFEST_NAME' && chmod 600 '$MANIFEST_NAME'"
ok "manifest written"

# ── 6. initialise ────────────────────────────────────────────────────────────
step "6 · Setting up the agent"
rm_ssh "MARGINALIA_HOME='$MARGINALIA_HOME' '$MARGINALIA_HOME/bin/${BINARY}' init" \
  || die "the agent could not initialise" \
         "Run ./tools/device/reset.sh to remove it cleanly."
ok "database created"

# ── done ─────────────────────────────────────────────────────────────────────
say ""
say "${C_BOLD}Installed.${C_OFF}"
say ""
say "  On your reMarkable, everything Marginalia has lives in one place:"
say "      ${MARGINALIA_HOME}"
say ""
say "  Try it:"
say "      ssh ${RM_USER}@${RM_HOST} '${MARGINALIA_HOME}/bin/${BINARY} status'"
say ""
say "  Connect your Zotero library:"
say "      see docs/INSTALL_REMARKABLE.md"
say ""
say "  To remove it completely and return your reMarkable to stock:"
say "      ./tools/device/reset.sh"
say ""
