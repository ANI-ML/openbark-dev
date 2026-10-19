#!/usr/bin/env bash
# A temporary keychain holding the Developer ID certificate, for one CI job
# (ADR-0020; the pattern in Tauri's macOS signing guide).
#
#   scripts/ci/macos-keychain.sh setup     # needs APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD
#   scripts/ci/macos-keychain.sh cleanup   # always run it, even after a failure
#
# APPLE_CERTIFICATE is the base64 text of a .p12. The .p12 exists on disk only
# for the moment it takes to import, inside $RUNNER_TEMP. The keychain's own
# password is random and never leaves this job. If APPLE_SIGNING_IDENTITY is
# set, setup checks that the keychain holds exactly that identity.
# Tauri and scripts/macos-dmg.sh then both find the identity through the
# keychain search list, so APPLE_CERTIFICATE must not be passed to the build.
set -euo pipefail
umask 077

TMP="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
KC="$TMP/openbark-signing.keychain-db"

case "${1:-}" in
  setup)
    [ -n "${APPLE_CERTIFICATE:-}" ] || { echo "APPLE_CERTIFICATE is empty."; exit 1; }
    [ -n "${APPLE_CERTIFICATE_PASSWORD:-}" ] || { echo "APPLE_CERTIFICATE_PASSWORD is empty."; exit 1; }
    kc_pass="$(/usr/bin/openssl rand -base64 32)"
    security delete-keychain "$KC" 2>/dev/null || true
    security create-keychain -p "$kc_pass" "$KC"
    # Stay unlocked for 6 hours: longer than bundling, signing and Apple's
    # notarization waits.
    security set-keychain-settings -lut 21600 "$KC"
    security unlock-keychain -p "$kc_pass" "$KC"
    p12="$(mktemp "$TMP/devid.XXXXXX")"
    trap 'rm -f "$p12"' EXIT
    printf '%s' "$APPLE_CERTIFICATE" | base64 --decode > "$p12"
    # -x: the private key can sign but can't be exported, by any process.
    # Only codesign is trusted to use it.
    security import "$p12" -k "$KC" -f pkcs12 -P "$APPLE_CERTIFICATE_PASSWORD" \
      -x -T /usr/bin/codesign >/dev/null
    rm -f "$p12"
    security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$kc_pass" "$KC" >/dev/null
    # Search this keychain first, then the existing ones; make it the default.
    existing="$(security list-keychains -d user | sed -e 's/^[[:space:]]*"//' -e 's/"$//')"
    # shellcheck disable=SC2086
    security list-keychains -d user -s "$KC" $existing
    security default-keychain -d user -s "$KC"
    ids="$(security find-identity -v -p codesigning "$KC")"
    printf '%s\n' "$ids" | sed -n '/valid identities found/p'
    if [ -n "${APPLE_SIGNING_IDENTITY:-}" ] && ! printf '%s\n' "$ids" | grep -Fq "\"$APPLE_SIGNING_IDENTITY\""; then
      echo "The certificate in APPLE_CERTIFICATE isn't a valid code-signing identity named APPLE_SIGNING_IDENTITY."
      printf '%s\n' "$ids"
      exit 1
    fi
    echo "Temporary keychain ready."
    ;;
  cleanup)
    if [ -f "$KC" ]; then
      security delete-keychain "$KC" || true
    fi
    login="$HOME/Library/Keychains/login.keychain-db"
    if [ -f "$login" ]; then
      security list-keychains -d user -s "$login" || true
      security default-keychain -d user -s "$login" || true
    fi
    echo "Temporary keychain removed."
    ;;
  *)
    sed -n '2,13p' "$0"
    exit 2
    ;;
esac
