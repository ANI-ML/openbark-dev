#!/usr/bin/env bash
# Check a signed OpenBark disk image (ADR-0020, decision 7). Fails on the
# first thing that's wrong.
#
#   APPLE_SIGNING_IDENTITY="Developer ID Application: ..." \
#     scripts/ci/macos-verify.sh <OpenBark.dmg> [--notarized]
#
# Always: both apps pass `codesign --verify --deep --strict`; every Mach-O file
# inside them is signed by APPLE_SIGNING_IDENTITY; the executables use the
# hardened runtime; the app has the microphone entitlement; the disk image is
# signed by the same identity.
# With --notarized (releases): every signature also carries a secure
# timestamp, Gatekeeper (spctl) accepts both apps and the disk image as
# notarized Developer ID code, and `stapler validate` passes on all three.
set -euo pipefail

DMG="${1:-}"
NOTARIZED=0
[ "${2:-}" = "--notarized" ] && NOTARIZED=1
[ -f "$DMG" ] || { echo "usage: macos-verify.sh <dmg> [--notarized]"; exit 2; }
ID="${APPLE_SIGNING_IDENTITY:-}"
[ -n "$ID" ] && [ "$ID" != "-" ] || { echo "APPLE_SIGNING_IDENTITY must name the expected identity."; exit 2; }

MNT="$(mktemp -d "${RUNNER_TEMP:-/tmp}/obdmg.XXXXXX")"
cleanup() { hdiutil detach "$MNT" -quiet 2>/dev/null || hdiutil detach "$MNT" -force -quiet 2>/dev/null || true; rmdir "$MNT" 2>/dev/null || true; }
trap cleanup EXIT
fail() { echo "::error::$*"; exit 1; }

# signed_by <path>: the leaf authority must be the expected identity, with a
# secure (server) timestamp.
signed_by() {
  local info
  info="$(codesign -dvv "$1" 2>&1)" || fail "not signed: $1"
  printf '%s\n' "$info" | grep -m1 '^Authority=' | grep -Fqx "Authority=$ID" \
    || fail "signed by someone else: $1 ($(printf '%s\n' "$info" | grep -m1 '^Authority=' || echo 'no authority, ad-hoc?'))"
  if ! printf '%s\n' "$info" | grep -q '^Timestamp='; then
    # Developer ID signatures get Apple's secure timestamp by default; a
    # self-signed test certificate doesn't, so only release checks insist.
    if [ "$NOTARIZED" = 1 ]; then fail "no secure timestamp: $1"; fi
    echo "  (no secure timestamp: ${1#"$MNT"/})"
  fi
}

hdiutil attach "$DMG" -nobrowse -readonly -noautoopen -mountpoint "$MNT" -quiet
APP="$MNT/OpenBark.app"
UNI="$MNT/Uninstall OpenBark.app"
[ -d "$APP" ] || fail "OpenBark.app is missing from the disk image"
[ -d "$UNI" ] || fail "Uninstall OpenBark.app is missing from the disk image"

for bundle in "$APP" "$UNI"; do
  name="$(basename "$bundle")"
  codesign --verify --deep --strict --verbose=2 "$bundle" || fail "codesign --verify failed: $name"
  signed_by "$bundle"
  codesign -dv "$bundle" 2>&1 | grep -q '^CodeDirectory.*flags=.*runtime' || fail "no hardened runtime: $name"
  # Every Mach-O inside (main executables, helpers, any dylib).
  count=0
  while IFS= read -r -d '' f; do
    if file -b "$f" | grep -q 'Mach-O'; then
      count=$((count + 1))
      codesign --verify --strict "$f" || fail "nested code fails to verify: ${f#"$MNT"/}"
      signed_by "$f"
      echo "  ok  ${f#"$MNT"/}"
    fi
  done < <(find "$bundle" -type f -print0)
  [ "$count" -gt 0 ] || fail "no Mach-O code found in $name"
  echo "ok  $name: $count Mach-O file(s), all signed by the expected identity"
done

codesign -d --entitlements - --xml "$APP" 2>/dev/null | grep -q 'com.apple.security.device.audio-input' \
  || fail "OpenBark.app lacks the com.apple.security.device.audio-input entitlement"
echo "ok  microphone entitlement"

codesign --verify --strict --verbose=2 "$DMG" || fail "the disk image's signature doesn't verify"
signed_by "$DMG"
echo "ok  disk image signature"

if [ "$NOTARIZED" = 1 ]; then
  for bundle in "$APP" "$UNI"; do
    out="$(spctl -a -t exec -vv "$bundle" 2>&1)" || { printf '%s\n' "$out"; fail "Gatekeeper rejects $(basename "$bundle")"; }
    printf '%s\n' "$out"
    printf '%s\n' "$out" | grep -q 'source=Notarized Developer ID' || fail "not notarized: $(basename "$bundle")"
    xcrun stapler validate "$bundle" || fail "no stapled ticket: $(basename "$bundle")"
  done
  out="$(spctl -a -t open --context context:primary-signature -vv "$DMG" 2>&1)" || { printf '%s\n' "$out"; fail "Gatekeeper rejects the disk image"; }
  printf '%s\n' "$out"
  printf '%s\n' "$out" | grep -q 'source=Notarized Developer ID' || fail "the disk image isn't notarized"
  xcrun stapler validate "$DMG" || fail "no stapled ticket on the disk image"
  echo "ok  notarized and stapled: app, uninstaller, disk image"
else
  echo "Notarization not checked (self-test)."
fi
echo "macOS signature checks passed."
