#!/usr/bin/env bash
# Build the iOS app for the simulator, unsigned (ADR-0018 section 8, the iOS
# check job). macOS with Xcode only. The Tauri CLI must run the build: the
# Xcode project's "Build Rust Code" phase talks to it. Prints the path of the
# .app on its last line and writes it to $GITHUB_OUTPUT as `app` when set.
#   bash scripts/ci/ios-sim-build.sh [cargo features, comma separated; default stt,inference]
#
# The phone records, transcribes and, on 8 GB iPhones that opt in, drafts
# (ADR-0018 sections 1 and 10): `stt` and `inference` (llama.cpp, Metal, no
# OpenMP) are on unless other features are named. ios-prepare.sh fetches sherpa-onnx's iOS library
# and links it into the Xcode project first.
set -euo pipefail
cd "$(dirname "$0")/../.."
features="${1:-stt,inference}"
SHERPA_ONNX_ARCHIVE_DIR="$(bash scripts/ci/ios-prepare.sh | tail -1)"
export SHERPA_ONNX_ARCHIVE_DIR
desktop=apps/desktop
gen="$desktop/src-tauri/gen/apple"
[ -d "$gen" ] || { echo "no $gen: run the iOS init job first (or tauri ios init)" >&2; exit 1; }

args=(tauri ios build --target aarch64-sim --debug --ci)
[ -n "$features" ] && args+=(--features "$features")
echo "== pnpm ${args[*]}"
(cd "$desktop" && pnpm "${args[@]}")

# The CLI puts the simulator bundle in gen/apple/build/arm64-sim/<name>.app.
app="$(find "$gen/build" -maxdepth 3 -type d -name '*.app' 2>/dev/null | grep -i 'sim' | head -1 || true)"
[ -n "$app" ] || { echo "no simulator .app was built" >&2; find "$gen/build" -maxdepth 3 >&2 || true; exit 1; }
[ -n "${GITHUB_OUTPUT:-}" ] && echo "app=$app" >> "$GITHUB_OUTPUT"
echo "$app"
