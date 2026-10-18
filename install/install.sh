#!/usr/bin/env bash
# Install OpenBark for the current user on macOS or Linux (no sudo needed).
#
#   ./install/install.sh [--from <folder>] [--models] [--gpu auto|metal|cuda|vulkan|cpu]
#
# Where the program comes from, in order:
#   1. --from <folder>: a release folder holding `openbark` and `ui/`
#   2. the folder this script is in, if it holds `openbark` and `ui/` (an unpacked release)
#   3. a build from source, when run from a checkout of the OpenBark repository
#
# Installs to:
#   macOS: ~/Applications/OpenBark.app (opens OpenBark in your browser)
#   Linux: ~/.local/opt/openbark, with an app-menu entry
# and links `openbark` into ~/.local/bin for the terminal.
#
# Your encounters, models and settings are never touched by installing or updating:
#   macOS: ~/Library/Application Support/OpenBark    Linux: ~/.local/share/OpenBark
set -euo pipefail

FROM=""
MODELS=0
GPU="auto"
while [ $# -gt 0 ]; do
  case "$1" in
    --from) FROM="$2"; shift 2 ;;
    --models) MODELS=1; shift ;;
    --gpu) GPU="$2"; shift 2 ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1 (try --help)"; exit 2 ;;
  esac
done

step() { printf '\033[36m==> %s\033[0m\n' "$1"; }
fail() { printf '\033[31mOpenBark was not installed: %s\033[0m\n' "$1"; exit 1; }

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(dirname "$HERE")"
OS="$(uname -s)"
case "$OS" in
  Darwin)
    APP_BUNDLE="$HOME/Applications/OpenBark.app"
    APP_DIR="$APP_BUNDLE/Contents/Resources"
    DATA_DIR="$HOME/Library/Application Support/OpenBark"
    ;;
  Linux)
    APP_DIR="$HOME/.local/opt/openbark"
    DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/OpenBark"
    ;;
  *) fail "this script is for macOS and Linux. On Windows, double-click 'Install OpenBark.cmd'." ;;
esac
BIN_LINK="$HOME/.local/bin/openbark"

is_release() { [ -n "$1" ] && [ -x "$1/openbark" ] && [ -f "$1/ui/index.html" ]; }

# 1. Find or build the program.
if [ -n "$FROM" ]; then
  is_release "$FROM" || fail "$FROM doesn't contain openbark and ui/index.html."
  SOURCE="$(cd "$FROM" && pwd)"
elif is_release "$HERE"; then
  SOURCE="$HERE"
elif [ -f "$REPO/Cargo.toml" ]; then
  step "Building OpenBark from source (the first build takes 10 to 20 minutes)"
  for tool in cargo node pnpm; do
    command -v "$tool" >/dev/null 2>&1 || fail "$tool isn't installed. See docs/runbooks for the one-time setup, or install from a release."
  done
  if [ "$GPU" = "auto" ]; then
    if [ "$OS" = "Darwin" ]; then
      if [ "$(uname -m)" = "arm64" ]; then GPU="metal"; else GPU="cpu"; fi
    elif command -v nvcc >/dev/null 2>&1; then GPU="cuda"
    elif [ -n "${VULKAN_SDK:-}" ] || command -v vulkaninfo >/dev/null 2>&1; then GPU="vulkan"
    else GPU="cpu"; fi
  fi
  echo "    graphics: $GPU"
  FEATURES=()
  [ "$GPU" != "cpu" ] && FEATURES=(--features "$GPU")
  (cd "$REPO" && cargo build --release -p os-cli ${FEATURES[@]+"${FEATURES[@]}"}) || fail "the engine didn't build (see the messages above)."
  (cd "$REPO" && pnpm install --frozen-lockfile && pnpm --filter @openbark/desktop build) || fail "the interface didn't build."
  SOURCE="$(mktemp -d)"
  mkdir -p "$SOURCE/ui"
  cp "$REPO/target/release/openbark" "$SOURCE/"
  # The speech engine's libraries (sherpa-onnx, ONNX Runtime) sit beside the program.
  find "$REPO/target/release" -maxdepth 1 \( -name '*.dylib' -o -name '*.so' -o -name '*.so.*' \) -exec cp -P {} "$SOURCE/" \;
  cp -R "$REPO/apps/desktop/dist/." "$SOURCE/ui/"
  cp "$REPO/apps/desktop/src-tauri/icons/128x128.png" "$SOURCE/icon.png" 2>/dev/null || true
  cp "$REPO/apps/desktop/src-tauri/icons/icon.icns" "$SOURCE/icon.icns" 2>/dev/null || true
else
  fail "no OpenBark program found. Run this from an unpacked release, pass --from <folder>, or run it from the source code."
fi

# 2. Stop a running copy so its files can be replaced.
pkill -f "$APP_DIR/openbark serve" 2>/dev/null || true

# 3. Copy the program.
step "Installing to ${APP_BUNDLE:-$APP_DIR}"
mkdir -p "$APP_DIR"
rm -rf "$APP_DIR/ui"
cp "$SOURCE/openbark" "$APP_DIR/openbark"
find "$SOURCE" -maxdepth 1 \( -name '*.dylib' -o -name '*.so' -o -name '*.so.*' \) -exec cp -P {} "$APP_DIR/" \;
cp -R "$SOURCE/ui" "$APP_DIR/ui"
[ -f "$SOURCE/icon.png" ] && cp "$SOURCE/icon.png" "$APP_DIR/icon.png"
cp "$HERE/uninstall.sh" "$APP_DIR/uninstall.sh" 2>/dev/null || true
chmod +x "$APP_DIR/openbark" "$APP_DIR/uninstall.sh" 2>/dev/null || true

# 4. Launcher: starts OpenBark on this computer only (127.0.0.1) and opens the browser.
cat > "$APP_DIR/openbark-launch" <<'EOF'
#!/usr/bin/env bash
DIR="$(cd "$(dirname "$0")" && pwd)"
[ -x "$DIR/openbark" ] || DIR="$(cd "$DIR/../Resources" && pwd)"
export LD_LIBRARY_PATH="$DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export DYLD_LIBRARY_PATH="$DIR${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"
URL="http://localhost:4317"
up() { curl -fsS -m 1 "$URL/api/status" >/dev/null 2>&1; }
if ! up; then
  LOG="${TMPDIR:-/tmp}/openbark.log"
  nohup "$DIR/openbark" serve >"$LOG" 2>&1 &
  for _ in $(seq 1 60); do up && break; sleep 0.5; done
fi
if command -v open >/dev/null 2>&1; then open "$URL"; else xdg-open "$URL" >/dev/null 2>&1 & fi
EOF
chmod +x "$APP_DIR/openbark-launch"

# 5. App entry.
if [ "$OS" = "Darwin" ]; then
  mkdir -p "$APP_BUNDLE/Contents/MacOS"
  cp "$APP_DIR/openbark-launch" "$APP_BUNDLE/Contents/MacOS/OpenBark"
  [ -f "$SOURCE/icon.icns" ] && cp "$SOURCE/icon.icns" "$APP_DIR/icon.icns"
  cat > "$APP_BUNDLE/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>OpenBark</string>
  <key>CFBundleDisplayName</key><string>OpenBark</string>
  <key>CFBundleIdentifier</key><string>health.animl.openbark.launcher</string>
  <key>CFBundleExecutable</key><string>OpenBark</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSUIElement</key><true/>
</dict>
</plist>
EOF
  # A release downloaded from the internet is quarantined; this copy was checked by you.
  xattr -dr com.apple.quarantine "$APP_BUNDLE" 2>/dev/null || true
else
  APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
  mkdir -p "$APPS"
  cat > "$APPS/openbark.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=OpenBark
Comment=Local veterinary scribe (opens in your browser)
Exec="$APP_DIR/openbark-launch"
Icon=$APP_DIR/icon.png
Terminal=false
Categories=Office;MedicalSoftware;
EOF
  cat > "$APPS/openbark-stop.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Stop OpenBark
Exec=pkill -f "$APP_DIR/openbark serve"
Icon=$APP_DIR/icon.png
Terminal=false
NoDisplay=false
Categories=Office;
EOF
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" >/dev/null 2>&1 || true
fi

# 6. Terminal command.
mkdir -p "$(dirname "$BIN_LINK")"
rm -f "$BIN_LINK"
cat > "$BIN_LINK" <<EOF
#!/usr/bin/env bash
# OpenBark terminal command (installed by install.sh)
export LD_LIBRARY_PATH="$APP_DIR\${LD_LIBRARY_PATH:+:\$LD_LIBRARY_PATH}"
export DYLD_LIBRARY_PATH="$APP_DIR\${DYLD_LIBRARY_PATH:+:\$DYLD_LIBRARY_PATH}"
exec "$APP_DIR/openbark" "\$@"
EOF
chmod +x "$BIN_LINK"
case ":$PATH:" in
  *":$HOME/.local/bin:"*) ;;
  *) echo "    Add ~/.local/bin to your PATH to use 'openbark' in a terminal." ;;
esac

# 7. Models (optional; the app can download them too).
if [ "$MODELS" = 1 ]; then
  step "Downloading models (about 6 GB; resumable if interrupted)"
  "$BIN_LINK" models setup || echo "Some models didn't download. Open OpenBark > Models to finish."
fi

printf '\n\033[32mOpenBark is installed.\033[0m\n'
if [ "$OS" = "Darwin" ]; then
  echo "  Open it from ~/Applications/OpenBark (it opens in your browser, on this computer only)."
else
  echo "  Open it from your app menu, or run: $APP_DIR/openbark-launch"
fi
echo "  Your data: $DATA_DIR"
echo "  To uninstall: $APP_DIR/uninstall.sh"
