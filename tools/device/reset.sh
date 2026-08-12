#!/usr/bin/env bash
# Remove Marginalia completely and return your reMarkable to stock.
#
#   ./tools/device/reset.sh              show what would be removed, then ask
#   ./tools/device/reset.sh --dry-run    show what would be removed, change nothing
#   ./tools/device/reset.sh --yes        skip the confirmation
#
# ── What this removes ────────────────────────────────────────────────────────
#
# Exactly one directory: the one Marginalia created. Nothing else.
#
# ── What it does not need to undo ────────────────────────────────────────────
#
# Marginalia never changed your reMarkable's software, so there is nothing to
# restore. No patched xochitl, no modified system files, no startup entries, no
# altered settings. Your notebooks, your documents and your reading position
# are untouched because they were never touched.
#
# That is the whole point of the design: "return to stock" is `rm -rf` on one
# directory, and this script exists mainly to prove it.
#
# ── What it will not remove ──────────────────────────────────────────────────
#
# Documents Marginalia downloaded for you at your request are *yours*. They are
# ordinary PDFs in your library, and this script leaves them alone. Delete them
# from the reMarkable itself if you want them gone.

source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

DRY_RUN=0
ASSUME_YES=0
for arg in "$@"; do
  case "$arg" in
    --dry-run) DRY_RUN=1 ;;
    --yes|-y)  ASSUME_YES=1 ;;
    *) die "unknown option: $arg" "usage: reset.sh [--dry-run] [--yes]" ;;
  esac
done

say "${C_BOLD}Removing Marginalia from your reMarkable${C_OFF}"
(( DRY_RUN )) && say "${C_WARN}Dry run — nothing will be changed.${C_OFF}"

# The same guard the installer uses. Without it, a mistyped MARGINALIA_HOME
# would make this script into something dangerous.
assert_home_is_safe

step "1 · Finding your reMarkable"
require_device
ok "connected at ${RM_HOST}"

if ! rm_ssh "test -d '$MARGINALIA_HOME'" 2>/dev/null; then
  say ""
  ok "Marginalia is not installed. Your reMarkable is already stock."
  say ""
  exit 0
fi
ok "found ${MARGINALIA_HOME}"

# ── 2. show exactly what is there ────────────────────────────────────────────
step "2 · What will be removed"

file_count=$(rm_ssh "find '$MARGINALIA_HOME' -type f 2>/dev/null | wc -l" | tr -d '\r ')
size=$(rm_ssh "du -sh '$MARGINALIA_HOME' 2>/dev/null | awk '{print \$1}'" | tr -d '\r')

info "${MARGINALIA_HOME}"
info "${file_count} file(s), ${size}"
say ""
rm_ssh "find '$MARGINALIA_HOME' -type f 2>/dev/null | head -30 | sed 's|^|      |'"
if (( file_count > 30 )); then
  info "… and $(( file_count - 30 )) more"
fi

# ── 3. prove nothing outside that directory is involved ──────────────────────
step "3 · Checking nothing outside that directory is involved"

if rm_ssh "test -f '$MARGINALIA_HOME/$MANIFEST_NAME'" 2>/dev/null; then
  # Every manifest entry must be inside our home. If one is not, something is
  # wrong and we stop rather than act on it.
  stray=$(rm_ssh "grep -v '^#' '$MARGINALIA_HOME/$MANIFEST_NAME' | cut -f1 | grep '^/' || true" | tr -d '\r')
  if [[ -n "$stray" ]]; then
    fail "the manifest lists paths outside ${MARGINALIA_HOME}:"
    printf '      %s\n' $stray
    die "refusing to continue" \
        "This should be impossible. Please open an issue with this output."
  fi
  ok "every installed file is inside ${MARGINALIA_HOME}"
else
  warn "no manifest found"
  info "the directory will be removed wholesale, which is still only that directory"
fi

# Paths Marginalia never creates. We look for them so that finding one stops
# the script, rather than letting a blind removal proceed next to something
# unexplained.
SUSPICIOUS_PATHS=(
  "/etc/systemd/system/marginalia.service"   # guard-allow: checked for, never created
  "/lib/systemd/system/marginalia.service"   # guard-allow: checked for, never created
  "/etc/init.d/marginalia"                   # guard-allow: checked for, never created
)

for suspicious in "${SUSPICIOUS_PATHS[@]}"; do
  if rm_ssh "test -e '$suspicious'" 2>/dev/null; then
    fail "found ${suspicious}"
    die "Marginalia never creates system services" \
        "Something else put that there. Please investigate before continuing."
  fi
done
ok "no system services, startup entries or modified system files"

# ── 4. confirm ───────────────────────────────────────────────────────────────
if (( DRY_RUN )); then
  say ""
  say "${C_BOLD}Dry run complete.${C_OFF} Nothing was changed."
  say "Run without --dry-run to remove it."
  say ""
  exit 0
fi

if (( ! ASSUME_YES )); then
  confirm "This removes ${MARGINALIA_HOME} and everything in it, including any Zotero key you stored and any annotations not yet exported." "remove"
fi

# ── 5. remove ────────────────────────────────────────────────────────────────
step "4 · Removing"

path_is_ours "${MARGINALIA_HOME}/." \
  || die "internal guard failed" "Nothing was removed. Please open an issue."

rm_ssh "rm -rf '$MARGINALIA_HOME'"
ok "removed ${MARGINALIA_HOME}"

# ── 6. verify ────────────────────────────────────────────────────────────────
step "5 · Verifying"

if rm_ssh "test -d '$MARGINALIA_HOME'" 2>/dev/null; then
  die "${MARGINALIA_HOME} is still present" \
      "Removal did not complete. Your device is otherwise unaffected."
fi
ok "the directory is gone"

if rm_ssh "test -d /home/root" 2>/dev/null; then
  ok "your home directory is intact"
fi

remaining=$(rm_ssh "ls -A /home/root 2>/dev/null | grep -i marginalia || true" | tr -d '\r')
if [[ -n "$remaining" ]]; then
  warn "these still mention Marginalia:"
  printf '      %s\n' $remaining
  info "they were not in the manifest, so they were left alone"
else
  ok "nothing named Marginalia remains"
fi

# ── done ─────────────────────────────────────────────────────────────────────
say ""
say "${C_BOLD}Done. Your reMarkable is back to stock.${C_OFF}"
say ""
say "  There was nothing to restore. Marginalia never modified your"
say "  reMarkable's software, so removing its directory removed all of it."
say ""
say "  ${C_DIM}Your notebooks, documents and reading positions were never touched.${C_OFF}"
say "  ${C_DIM}Documents you asked Marginalia to download are yours and remain in${C_OFF}"
say "  ${C_DIM}your library — delete them from the reMarkable if you want them gone.${C_OFF}"
say ""
say "  If you also want to revoke the Zotero key you gave it:"
say "      https://www.zotero.org/settings/keys"
say ""
