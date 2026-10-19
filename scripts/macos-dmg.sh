#!/usr/bin/env bash
# Build OpenBark's macOS disk image: the app, a drag-to-Applications link and
# "Uninstall OpenBark", laid out in one Finder window.
#
#   scripts/macos-dmg.sh [--features metal] [--skip-build] [--no-notarize]
#
# Runs on a Mac (the release workflow's macos-15 job, or the product owner's
# Mac). Builds the app with `pnpm tauri build --bundles app` unless
# --skip-build, then writes target/release/bundle/dmg/OpenBark_<version>_<arch>.dmg.
# create-dmg is downloaded at a pinned version (docs/versions.md).
#
# Signing (ADR-0020): with APPLE_SIGNING_IDENTITY set to a Developer ID (not
# "-"), the identity must already be in a keychain (scripts/ci/macos-keychain.sh).
# Tauri then signs the app with the hardened runtime and Entitlements.plist,
# notarizes and staples it; this script signs, notarizes and staples the
# uninstaller and the disk image. Notarizing needs APPLE_API_KEY,
# APPLE_API_ISSUER and APPLE_API_KEY_PATH (an App Store Connect .p8).
# --no-notarize signs without notarizing (the CI self-test's throwaway
# certificate only). Without APPLE_SIGNING_IDENTITY, everything is signed
# ad-hoc, as local builds always are.
set -euo pipefail

FEATURES="metal"
BUILD=1
NOTARIZE=1
while [ $# -gt 0 ]; do
  case "$1" in
    --features) FEATURES="$2"; shift 2 ;;
    --skip-build) BUILD=0; shift ;;
    --no-notarize) NOTARIZE=0; shift ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1 (try --help)"; exit 2 ;;
  esac
done

[ "$(uname -s)" = Darwin ] || { echo "This builds the macOS disk image, so it runs on a Mac."; exit 1; }

SIGN_ID="${APPLE_SIGNING_IDENTITY:-}"
SIGNING=0
if [ -n "$SIGN_ID" ] && [ "$SIGN_ID" != "-" ]; then
  SIGNING=1
else
  # Tauri treats a set-but-empty APPLE_SIGNING_IDENTITY as the identity ""
  # instead of the committed "-" (the release workflow sets it empty when
  # not signing), and would try to notarize with any APPLE_API_* present.
  unset APPLE_SIGNING_IDENTITY APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH \
    APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD
fi
if [ "$SIGNING" = 1 ] && [ "$NOTARIZE" = 1 ]; then
  for v in APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH; do
    [ -n "${!v:-}" ] || { echo "Signing with a Developer ID needs $v for notarization (or pass --no-notarize)."; exit 1; }
  done
  [ -f "$APPLE_API_KEY_PATH" ] || { echo "APPLE_API_KEY_PATH doesn't point at a file."; exit 1; }
fi
if [ "$SIGNING" = 1 ]; then
  echo "==> Signing mode: Developer ID$( [ "$NOTARIZE" = 1 ] && echo ", notarized and stapled" || echo ", not notarized (--no-notarize)")"
else
  echo "==> Ad-hoc signing (no APPLE_SIGNING_IDENTITY)"
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CREATE_DMG_VERSION="1.3.0"
VERSION="$(node -p "require('$ROOT/apps/desktop/src-tauri/tauri.conf.json').version")"
case "$(uname -m)" in arm64) ARCH=aarch64 ;; *) ARCH=x64 ;; esac
ICNS="$ROOT/apps/desktop/src-tauri/icons/icon.icns"
OUT_DIR="$ROOT/target/release/bundle/dmg"
OUT="$OUT_DIR/OpenBark_${VERSION}_${ARCH}.dmg"
WORK="$ROOT/target/dmg-work"

# notarize <file>: submit to Apple's notary service and wait. On anything but
# Accepted, print Apple's log and stop.
notarize() {
  local file="$1" out id status
  echo "==> Notarizing $(basename "$file")"
  out="$(xcrun notarytool submit "$file" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" \
    --issuer "$APPLE_API_ISSUER" --wait --output-format json)" || true
  id="$(printf '%s' "$out" | /usr/bin/plutil -extract id raw -o - - 2>/dev/null || true)"
  status="$(printf '%s' "$out" | /usr/bin/plutil -extract status raw -o - - 2>/dev/null || true)"
  echo "    submission ${id:-?}: ${status:-no status}"
  if [ "$status" != "Accepted" ]; then
    printf '%s\n' "$out"
    if [ -n "$id" ]; then
      xcrun notarytool log "$id" --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY" --issuer "$APPLE_API_ISSUER" || true
    fi
    echo "Notarization of $(basename "$file") failed."
    exit 1
  fi
}

if [ "$BUILD" = 1 ]; then
  if [ "$SIGNING" = 1 ]; then
    # Compile first with no signing values in the environment, so npm and
    # cargo build scripts never see them. Then Tauri bundles and signs
    # (APPLE_SIGNING_IDENTITY) and, with APPLE_API_*, notarizes and staples.
    (cd "$ROOT/apps/desktop" && env -u APPLE_SIGNING_IDENTITY -u APPLE_API_KEY -u APPLE_API_ISSUER \
      -u APPLE_API_KEY_PATH -u APPLE_CERTIFICATE -u APPLE_CERTIFICATE_PASSWORD \
      pnpm tauri build --no-bundle --features "$FEATURES")
    if [ "$NOTARIZE" = 1 ]; then
      (cd "$ROOT/apps/desktop" && env -u APPLE_CERTIFICATE -u APPLE_CERTIFICATE_PASSWORD \
        pnpm tauri bundle --bundles app --features "$FEATURES")
    else
      (cd "$ROOT/apps/desktop" && env -u APPLE_CERTIFICATE -u APPLE_CERTIFICATE_PASSWORD \
        -u APPLE_API_KEY -u APPLE_API_ISSUER -u APPLE_API_KEY_PATH \
        pnpm tauri bundle --bundles app --features "$FEATURES")
    fi
  else
    (cd "$ROOT/apps/desktop" && pnpm tauri build --bundles app --features "$FEATURES")
  fi
fi
APP="$ROOT/target/release/bundle/macos/OpenBark.app"
[ -d "$APP" ] || { echo "No app at $APP: build it first (drop --skip-build)."; exit 1; }
if [ "$SIGNING" = 1 ]; then
  codesign --verify --deep --strict "$APP"
  if [ "$NOTARIZE" = 1 ]; then
    xcrun stapler validate "$APP"
  fi
fi

rm -rf "$WORK" && mkdir -p "$WORK/stage" "$OUT_DIR"

echo "==> create-dmg $CREATE_DMG_VERSION"
curl -fsSL "https://github.com/create-dmg/create-dmg/archive/refs/tags/v$CREATE_DMG_VERSION.tar.gz" | tar -xz -C "$WORK"
CREATE_DMG="$WORK/create-dmg-$CREATE_DMG_VERSION/create-dmg"

echo "==> Uninstall OpenBark.app"
UNINSTALLER="$WORK/Uninstall OpenBark.app"
cp -R "$ROOT/install/macos/Uninstall OpenBark.app" "$UNINSTALLER"
mkdir -p "$UNINSTALLER/Contents/Resources"
cp "$ICNS" "$UNINSTALLER/Contents/Resources/icon.icns"
chmod +x "$UNINSTALLER/Contents/MacOS/uninstall"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" -c "Set :CFBundleVersion $VERSION" "$UNINSTALLER/Contents/Info.plist"
if [ "$SIGNING" = 1 ]; then
  # A Mach-O main executable (install/macos/uninstall-launcher.c) runs the
  # script from Resources, because the notary service expects compiled code.
  mv "$UNINSTALLER/Contents/MacOS/uninstall" "$UNINSTALLER/Contents/Resources/uninstall.sh"
  xcrun clang -O2 -Wall -Werror -mmacosx-version-min=13.0 \
    -o "$UNINSTALLER/Contents/MacOS/uninstall" "$ROOT/install/macos/uninstall-launcher.c"
  /usr/libexec/PlistBuddy -c "Add :NSAppleEventsUsageDescription string Uninstall OpenBark asks OpenBark to quit before removing it." \
    -c "Add :NSHumanReadableCopyright string $(node -p "require('$ROOT/apps/desktop/src-tauri/tauri.conf.json').bundle.copyright")" \
    "$UNINSTALLER/Contents/Info.plist"
  codesign --force --options runtime --timestamp \
    --entitlements "$ROOT/install/macos/uninstaller.entitlements" \
    --sign "$SIGN_ID" "$UNINSTALLER"
  codesign --verify --deep --strict "$UNINSTALLER"
  if [ "$NOTARIZE" = 1 ]; then
    ditto -c -k --keepParent "$UNINSTALLER" "$WORK/uninstaller.zip"
    notarize "$WORK/uninstaller.zip"
    rm -f "$WORK/uninstaller.zip"
    # Staple before the disk image is made: the image is read-only afterwards.
    xcrun stapler staple "$UNINSTALLER"
    xcrun stapler validate "$UNINSTALLER"
  fi
else
  # Ad-hoc signature, like the app (tauri.macos.conf.json).
  codesign --force --sign - "$UNINSTALLER" || true
fi

# ditto keeps the signature and the stapled ticket exactly as they are.
ditto "$APP" "$WORK/stage/OpenBark.app"
rm -f "$OUT"

layout=(
  --volname "OpenBark"
  --volicon "$ICNS"
  --window-pos 200 120
  --window-size 640 440
  --icon-size 96
  --text-size 13
  --icon "OpenBark.app" 170 170
  --hide-extension "OpenBark.app"
  --app-drop-link 470 170
  --add-file "Uninstall OpenBark.app" "$UNINSTALLER" 320 330
  --hide-extension "Uninstall OpenBark.app"
  --hdiutil-retries 10
)
echo "==> $OUT"
# The Finder layout step uses AppleScript, which can time out on a busy CI Mac:
# then build the same image without the icon positions.
if ! "$CREATE_DMG" "${layout[@]}" "$OUT" "$WORK/stage"; then
  echo "Finder layout failed; building without it."
  rm -f "$OUT" "$OUT_DIR"/rw.*.dmg
  "$CREATE_DMG" "${layout[@]}" --skip-jenkins "$OUT" "$WORK/stage"
fi

if [ "$SIGNING" = 1 ]; then
  echo "==> Signing the disk image"
  codesign --force --timestamp --sign "$SIGN_ID" "$OUT"
  codesign --verify --strict "$OUT"
  if [ "$NOTARIZE" = 1 ]; then
    notarize "$OUT"
    xcrun stapler staple "$OUT"
    xcrun stapler validate "$OUT"
  fi
fi
echo "Done: $OUT"
