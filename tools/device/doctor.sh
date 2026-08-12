#!/usr/bin/env bash
# Read-only checks. Changes nothing, on your machine or on the device.
#
# Run this before installing, and again if anything looks wrong.

source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

say "${C_BOLD}Marginalia — device check${C_OFF}"
say "${C_DIM}Nothing is written. Every command below only reads.${C_OFF}"

assert_home_is_safe

step "Your machine"
if command -v cargo >/dev/null 2>&1; then
  ok "cargo $(cargo --version | awk '{print $2}')"
else
  fail "cargo not found — install Rust from https://rustup.rs"
fi

if command -v cross >/dev/null 2>&1; then
  ok "cross (builds for the reMarkable's ARM processor)"
elif command -v arm-linux-gnueabihf-gcc >/dev/null 2>&1; then
  ok "arm-linux-gnueabihf-gcc"
else
  warn "no ARM cross-compiler found"
  info "install one with: cargo install cross   (needs Docker)"
  info "without it, install.sh cannot build the agent for your device"
fi

step "Your reMarkable"
if rm_ssh true 2>/dev/null; then
  ok "reachable at ${RM_HOST}"
  info "firmware $(device_description)"

  free_kb=$(rm_ssh "df -k /home | tail -1 | awk '{print \$4}'" | tr -d '\r')
  if [[ -n "$free_kb" ]]; then
    info "$(( free_kb / 1024 )) MB free in /home"
    if (( free_kb < 512000 )); then
      warn "less than 500 MB free — Marginalia keeps a reserve and will refuse to fill your device"
    fi
  fi

  if rm_ssh "test -d '$MARGINALIA_HOME'" 2>/dev/null; then
    ok "Marginalia is installed at $MARGINALIA_HOME"
    if rm_ssh "test -f '$MARGINALIA_HOME/$MANIFEST_NAME'" 2>/dev/null; then
      count=$(rm_ssh "wc -l < '$MARGINALIA_HOME/$MANIFEST_NAME'" | tr -d '\r ')
      info "$count file(s) in the install manifest"
    else
      warn "installed, but no manifest — reset.sh will remove the directory wholesale"
    fi
    rm_ssh "'$MARGINALIA_HOME/bin/marginalia' doctor" 2>/dev/null || \
      warn "the agent did not report a clean state"
  else
    info "Marginalia is not installed"
  fi
else
  fail "cannot reach your reMarkable at ${RM_HOST}"
  info "connect it by USB, switch it on, and enable developer access"
  info "then check: ssh ${RM_USER}@${RM_HOST}"
fi

step "What Marginalia will and will not touch"
ok "writes only: $MARGINALIA_HOME"
say "  ${C_DIM}never: /usr /etc /lib /bin /boot /opt — the device's own software${C_OFF}"
say "  ${C_DIM}never: xochitl, the kernel, the bootloader, firmware updates${C_OFF}"
say "  ${C_DIM}never: your notebooks, or any document Marginalia did not create${C_OFF}"
say ""
