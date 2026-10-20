#!/usr/bin/env bash
# Build the iOS app for the simulator, unsigned (ADR-0018 section 8, the iOS
# check job). macOS with Xcode only. Prints the path of the .app on the last
# line of its output and writes it to $GITHUB_OUTPUT as `app` when set.
#   bash scripts/ci/ios-sim-build.sh [cargo features, comma separated]
set -euo pipefail
cd "$(dirname "$0")/../.."
features="${1:-}"
desktop=apps/desktop
gen="$desktop/src-tauri/gen/apple"
[ -d "$gen" ] || { echo "no $gen: run the iOS init job first (or tauri ios init)" >&2; exit 1; }

args=(tauri ios build --target aarch64-sim --debug --ci)
[ -n "$features" ] && args+=(--features "$features")

app=""
echo "== pnpm ${args[*]}"
if (cd "$desktop" && pnpm "${args[@]}"); then
  app="$(find "$gen/build" -type d -name '*.app' -path '*simulator*' 2>/dev/null | head -1 || true)"
fi

if [ -z "$app" ]; then
  # The CLI's simulator build either failed (it exports an archive, which
  # wants signing) or put the app somewhere else: build the Xcode project
  # directly, with signing off. Its "Build Rust Code" phase calls the CLI's
  # xcode-script, which builds the Rust library for the simulator.
  project="$(find "$gen" -maxdepth 1 -name '*.xcodeproj' | head -1)"
  scheme="$(basename "$project" .xcodeproj)_iOS"
  echo "== xcodebuild $project scheme $scheme (iphonesimulator, unsigned)"
  (cd "$desktop" && pnpm build)
  xcodebuild \
    -project "$project" -scheme "$scheme" -configuration debug \
    -sdk iphonesimulator -destination 'generic/platform=iOS Simulator' \
    -derivedDataPath "$gen/build/derived" ARCHS=arm64 ONLY_ACTIVE_ARCH=NO \
    CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO build
  app="$(find "$gen/build/derived" -type d -name '*.app' -path '*iphonesimulator*' | head -1)"
fi

[ -n "$app" ] || { echo "no simulator .app was built" >&2; exit 1; }
[ -n "${GITHUB_OUTPUT:-}" ] && echo "app=$app" >> "$GITHUB_OUTPUT"
echo "$app"
