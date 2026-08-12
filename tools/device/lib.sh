#!/usr/bin/env bash
# Shared helpers for the device scripts.
#
# Sourced, never run directly.
#
# The rules these enforce are the same ones the Rust code enforces, restated
# here because a shell script that talks to someone's reMarkable over SSH is
# exactly where a careless line does damage.

set -euo pipefail

# ── where the agent lives ────────────────────────────────────────────────────
# One directory. Removing it removes Marginalia entirely.
: "${MARGINALIA_HOME:=/home/root/.marginalia}"
: "${RM_HOST:=10.11.99.1}"
: "${RM_USER:=root}"

readonly MANIFEST_NAME="install-manifest.tsv"

# ── output ───────────────────────────────────────────────────────────────────
if [[ -t 1 ]] && [[ -z "${NO_COLOR:-}" ]]; then
  C_DIM=$'\033[2m'; C_BOLD=$'\033[1m'; C_OK=$'\033[32m'
  C_WARN=$'\033[33m'; C_ERR=$'\033[31m'; C_OFF=$'\033[0m'
else
  C_DIM=""; C_BOLD=""; C_OK=""; C_WARN=""; C_ERR=""; C_OFF=""
fi

say()   { printf '%s\n' "$*"; }
step()  { printf '\n%s%s%s\n' "$C_BOLD" "$*" "$C_OFF"; }
ok()    { printf '  %s✓%s %s\n' "$C_OK" "$C_OFF" "$*"; }
info()  { printf '  %s·%s %s\n' "$C_DIM" "$C_OFF" "$*"; }
warn()  { printf '  %s!%s %s\n' "$C_WARN" "$C_OFF" "$*"; }
fail()  { printf '  %s✗%s %s\n' "$C_ERR" "$C_OFF" "$*" >&2; }

die() {
  printf '\n%serror%s %s\n' "$C_ERR" "$C_OFF" "$1" >&2
  [[ $# -gt 1 ]] && printf '      %s\n' "$2" >&2
  exit 1
}

# ── the guard rails ──────────────────────────────────────────────────────────

# Every path this tooling writes must be inside MARGINALIA_HOME. Checked here
# rather than assumed, because the cost of being wrong is someone's device.
readonly FORBIDDEN_PREFIXES=(/usr /etc /lib /bin /sbin /boot /opt /var /proc /sys)

assert_home_is_safe() {
  local home="$MARGINALIA_HOME"

  [[ "$home" == /* ]] || die "MARGINALIA_HOME must be an absolute path" "got: $home"

  for prefix in "${FORBIDDEN_PREFIXES[@]}"; do
    if [[ "$home" == "$prefix"/* || "$home" == "$prefix" ]]; then
      die "MARGINALIA_HOME is inside $prefix, which belongs to the device" \
          "Marginalia only ever writes to its own directory."
    fi
  done

  case "$home" in
    / | /home | /home/root | /home/root/ )
      die "MARGINALIA_HOME is too broad: $home" \
          "It needs its own directory, so that removing it removes only Marginalia." ;;
  esac
}

# A path is ours only if it is under MARGINALIA_HOME. Used before every removal.
path_is_ours() {
  local path="$1"
  [[ "$path" == "$MARGINALIA_HOME"/* ]]
}

# ── talking to the device ────────────────────────────────────────────────────

# `-n` matters more than it looks. Without it ssh reads the script's own stdin
# and hands it to the remote command, so a check running before a prompt eats
# the answer the operator is about to give -- observed in reset.sh, where the
# safety checks in step 3 swallowed the confirmation and the removal was then
# refused for want of an answer that had already been typed. Failing closed was
# right, but the cause is this. No caller pipes local data to the device; the
# one redirect we use (`wc -l < file`) is evaluated on the device.
rm_ssh() {
  ssh -n -o ConnectTimeout=8 -o StrictHostKeyChecking=accept-new "${RM_USER}@${RM_HOST}" "$@"
}

rm_scp() {
  scp -o ConnectTimeout=8 -o StrictHostKeyChecking=accept-new "$@"
}

require_device() {
  if ! rm_ssh true 2>/dev/null; then
    die "cannot reach your reMarkable at ${RM_HOST}" \
"Check that:
        · the reMarkable is connected by USB and switched on
        · you enabled developer access (Settings → Help → About)
        · you can run: ssh ${RM_USER}@${RM_HOST}

      Set RM_HOST to use Wi-Fi instead, e.g. RM_HOST=192.168.1.42"
  fi
}

# Firmware release, for the compatibility record. Read-only.
#
# WHY not /etc/version: on a reMarkable 2 running 3.28 that file holds a build
# timestamp (20260806095513), not a release. Reporting it as the firmware makes
# every compatibility statement meaningless. The release lives in update.conf as
# a four-component string ("3.28.0.166"), with os-release's IMG_VERSION as a
# fallback for images that ship one and not the other. Verified on hardware.
device_description() {
  rm_ssh '
    v=$(sed -n "s/^REMARKABLE_RELEASE_VERSION=//p" /usr/share/remarkable/update.conf 2>/dev/null)
    [ -n "$v" ] || v=$(sed -n "s/^IMG_VERSION=\"\\(.*\\)\"/\\1/p" /etc/os-release 2>/dev/null)
    [ -n "$v" ] || v=unknown
    echo "$v"
  ' | tr -d '\r'
}

confirm() {
  local prompt="$1" expected="${2:-yes}" answer
  printf '\n%s%s%s\n' "$C_BOLD" "$prompt" "$C_OFF"
  printf 'Type %s to continue: ' "$expected"
  read -r answer
  [[ "$answer" == "$expected" ]] || die "cancelled — nothing was changed"
}
