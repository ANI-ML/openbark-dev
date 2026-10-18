#!/usr/bin/env bash
# Uninstall OpenBark for the current user on macOS or Linux.
#
#   uninstall.sh [--remove-data] [--yes]
#
# Removes the program, its app entry and the `openbark` terminal link. Your
# encounters, models and settings are kept unless you choose to delete them
# (asked when run without options; --remove-data --yes deletes without asking).
#
# Deleting data also deletes this computer's encryption key. Copies of
# encounters can then only be opened with your recovery key
# (Settings > Encryption > Show recovery key), so save it first.
set -uo pipefail

REMOVE=""
YES=0
while [ $# -gt 0 ]; do
  case "$1" in
    --remove-data) REMOVE=1; shift ;;
    --keep-data) REMOVE=0; shift ;;
    --yes) YES=1; shift ;;
    -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1 (try --help)"; exit 2 ;;
  esac
done

step() { printf '\033[36m==> %s\033[0m\n' "$1"; }

case "$(uname -s)" in
  Darwin)
    APP_BUNDLE="$HOME/Applications/OpenBark.app"
    APP_DIR="$APP_BUNDLE/Contents/Resources"
    DATA_DIR="$HOME/Library/Application Support/OpenBark"
    ;;
  *)
    APP_BUNDLE=""
    APP_DIR="$HOME/.local/opt/openbark"
    DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/OpenBark"
    ;;
esac
EXE="$APP_DIR/openbark"

# Run without options: offer to delete data too; keeping it is the default.
if [ "$REMOVE" != 0 ] && [ "$YES" = 0 ] && [ -t 0 ]; then
  printf '\033[33mDo you also want to delete everything OpenBark stored on this computer?\033[0m\n'
  echo "  $DATA_DIR (encounters, recordings, notes, models, templates, team members)"
  echo "  and the encryption key. Without your recovery key, copies of encounters can't be opened."
  read -r -p "Type DELETE to remove your data, or press Enter to keep it: " answer
  if [ "$answer" = "DELETE" ]; then REMOVE=1; else REMOVE=0; echo "Keeping your data."; fi
fi
[ "$REMOVE" = 1 ] || REMOVE=0

step "Stopping OpenBark"
pkill -f "$EXE serve" 2>/dev/null || true
sleep 0.5

if [ "$REMOVE" = 1 ] && [ -x "$EXE" ]; then
  step "Deleting the encryption key"
  LD_LIBRARY_PATH="$APP_DIR" DYLD_LIBRARY_PATH="$APP_DIR" "$EXE" key forget --yes || true
fi

step "Removing the program"
if [ -n "$APP_BUNDLE" ]; then
  rm -rf "$APP_BUNDLE"
else
  rm -rf "$APP_DIR"
  APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
  rm -f "$APPS/openbark.desktop" "$APPS/openbark-stop.desktop"
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" >/dev/null 2>&1 || true
fi
# Only remove the terminal command if this installer made it.
if [ -f "$HOME/.local/bin/openbark" ] && grep -q "installed by install.sh" "$HOME/.local/bin/openbark" 2>/dev/null; then
  rm -f "$HOME/.local/bin/openbark"
fi

if [ "$REMOVE" = 1 ]; then
  step "Deleting your data"
  rm -rf "$DATA_DIR"
  if [ -n "${OPENBARK_SESSIONS_DIR:-}${OPENBARK_MODELS_DIR:-}" ]; then
    echo "  OPENBARK_SESSIONS_DIR or OPENBARK_MODELS_DIR points elsewhere; those folders were left alone."
  fi
fi

printf '\n\033[32mOpenBark is uninstalled.\033[0m\n'
if [ "$REMOVE" = 0 ] && [ -d "$DATA_DIR" ]; then
  echo "  Your data is still in $DATA_DIR. Reinstall to use it again, or run with --remove-data to delete it."
fi
