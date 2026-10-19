#!/usr/bin/env bash
# Build OpenBark's macOS disk image: the app, a drag-to-Applications link and
# "Uninstall OpenBark", laid out in one Finder window.
#
#   scripts/macos-dmg.sh [--features metal] [--bundle-only | --skip-build] [--no-notarize]
#
# Runs on a Mac (the release workflow's macos-15 job, or the product owner's
# Mac). Builds the app with `pnpm tauri build --bundles app`, then writes
# target/release/bundle/dmg/OpenBark_<version>_<arch>.dmg. --bundle-only
# bundles an app already compiled with `pnpm tauri build --no-bundle`;
# --skip-build reuses target/release/bundle/macos/OpenBark.app as it is.
# create-dmg is downloaded at a pinned commit and checked by SHA-256
# (docs/versions.md).
#
# Signing (ADR-0020): with APPLE_SIGNING_IDENTITY set to a Developer ID (not
# "-"), the identity must already be in a keychain (scripts/ci/macos-keychain.sh).
# Tauri then signs the app with the hardened runtime and Entitlements.plist,
# notarizes and staples it; this script signs, notarizes and staples the
# uninstaller and the disk image. Notarizing needs APPLE_API_KEY,
# APPLE_API_ISSUER and APPLE_API_KEY_PATH (an App Store Connect .p8).
# In CI, signing needs --bundle-only: the workflow compiles in an earlier step,
# before the signing keychain and the .p8 exist, so build scripts can't reach
# them. --no-notarize signs without notarizing (the CI self-test's throwaway
# certificate only). Without APPLE_SIGNING_IDENTITY, everything is signed
# ad-hoc, as local builds always are.
set -euo pipefail

FEATURES="metal"
BUILD=1
COMPILE=1
NOTARIZE=1
while [ $# -gt 0 ]; do
  case "$1" in
    --features) FEATURES="$2"; shift 2 ;;
    --bundle-only) COMPILE=0; shift ;;
    --skip-build) BUILD=0; shift ;;
    --no-notarize) NOTARIZE=0; shift ;;
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
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
if [ "$SIGNING" = 1 ] && [ "$BUILD" = 1 ] && [ "$COMPILE" = 1 ] && [ -n "${CI:-}" ]; then
  echo "In CI, compile with 'pnpm tauri build --no-bundle' in an earlier step, before the signing keychain and the notarization key exist, then pass --bundle-only."
  exit 1
fi
if [ "$SIGNING" = 1 ]; then
  echo "==> Signing mode: Developer ID$( [ "$NOTARIZE" = 1 ] && echo ", notarized and stapled" || echo ", not notarized (--no-notarize)")"
else
  echo "==> Ad-hoc signing (no APPLE_SIGNING_IDENTITY)"
fi

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# create-dmg 1.3.0, by the tag's commit, and the SHA-256 of each file of it
# that runs here (docs/versions.md). Checked before it runs.
CREATE_DMG_VERSION="1.3.0"
CREATE_DMG_COMMIT="a2b71d0fda6d0df2a86dc7f67082d4d73e84c59f"
CREATE_DMG_SHA256="bb9ea3194e55f2f76a821e87541513748d0fedc69f45cf4f0951cad15ae0cae5  create-dmg
b5dd7c55ddaa5db1884ac5cf523c4413d452a75df967daf55b8d45ba501fe457  support/template.applescript
a804e533e9c99491a74cb4502c435b00d902dc7a45d3693057a29674e584a70b  support/eula-resources-template.xml"
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
  if [ "$SIGNING" = 1 ] || [ "$COMPILE" = 0 ]; then
    if [ "$COMPILE" = 1 ]; then
      # Signing on a Mac by hand: compile first with no signing values in the
      # environment. (The identity stays in the login keychain, as it always
      # does on that Mac. CI compiles before its keychain exists instead.)
      (cd "$ROOT/apps/desktop" && env -u APPLE_SIGNING_IDENTITY -u APPLE_API_KEY -u APPLE_API_ISSUER \
        -u APPLE_API_KEY_PATH -u APPLE_CERTIFICATE -u APPLE_CERTIFICATE_PASSWORD \
        pnpm tauri build --no-bundle --features "$FEATURES")
    fi
    # Tauri bundles and signs (APPLE_SIGNING_IDENTITY, or ad-hoc) and, with
    # APPLE_API_*, notarizes and staples.
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

echo "==> create-dmg $CREATE_DMG_VERSION ($CREATE_DMG_COMMIT)"
curl -fsSL "https://github.com/create-dmg/create-dmg/archive/$CREATE_DMG_COMMIT.tar.gz" | tar -xz -C "$WORK"
CREATE_DMG_DIR="$WORK/create-dmg-$CREATE_DMG_COMMIT"
printf '%s\n' "$CREATE_DMG_SHA256" | (cd "$CREATE_DMG_DIR" && /usr/bin/shasum -a 256 -c -) \
  || { echo "create-dmg doesn't match its pinned SHA-256 (docs/versions.md)."; exit 1; }
# Third-party code: it runs without the notarization key's variables.
create_dmg() {
  env -u APPLE_API_KEY -u APPLE_API_ISSUER -u APPLE_API_KEY_PATH -u APPLE_SIGNING_IDENTITY \
    "$CREATE_DMG_DIR/create-dmg" "$@"
}

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
if ! create_dmg "${layout[@]}" "$OUT" "$WORK/stage"; then
  echo "Finder layout failed; building without it."
  rm -f "$OUT" "$OUT_DIR"/rw.*.dmg
  create_dmg "${layout[@]}" --skip-jenkins "$OUT" "$WORK/stage"
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
