#!/usr/bin/env bash
# Build OpenBark's macOS disk image: the app, a drag-to-Applications link and
# "Uninstall OpenBark", laid out in one Finder window.
#
#   scripts/macos-dmg.sh [--features metal] [--skip-build]
#
# Runs on a Mac (the release workflow's macos-15 job, or the product owner's
# Mac). Builds the app with `pnpm tauri build --bundles app` unless
# --skip-build, then writes target/release/bundle/dmg/OpenBark_<version>_<arch>.dmg.
# create-dmg is downloaded at a pinned version (docs/versions.md).
set -euo pipefail

FEATURES="metal"
BUILD=1
while [ $# -gt 0 ]; do
  case "$1" in
    --features) FEATURES="$2"; shift 2 ;;
    --skip-build) BUILD=0; shift ;;
    -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1 (try --help)"; exit 2 ;;
  esac
done

[ "$(uname -s)" = Darwin ] || { echo "This builds the macOS disk image, so it runs on a Mac."; exit 1; }

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CREATE_DMG_VERSION="1.3.0"
VERSION="$(node -p "require('$ROOT/apps/desktop/src-tauri/tauri.conf.json').version")"
case "$(uname -m)" in arm64) ARCH=aarch64 ;; *) ARCH=x64 ;; esac
ICNS="$ROOT/apps/desktop/src-tauri/icons/icon.icns"
OUT_DIR="$ROOT/target/release/bundle/dmg"
OUT="$OUT_DIR/OpenBark_${VERSION}_${ARCH}.dmg"
WORK="$ROOT/target/dmg-work"

if [ "$BUILD" = 1 ]; then
  (cd "$ROOT/apps/desktop" && pnpm tauri build --bundles app --features "$FEATURES")
fi
APP="$ROOT/target/release/bundle/macos/OpenBark.app"
[ -d "$APP" ] || { echo "No app at $APP: build it first (drop --skip-build)."; exit 1; }

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
# Ad-hoc signature, like the app (tauri.macos.conf.json); a Developer ID is the product owner's call.
codesign --force --sign - "$UNINSTALLER" || true

cp -R "$APP" "$WORK/stage/"
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
echo "Done: $OUT"
