#!/usr/bin/env bash
# Type-check the engine for iOS from any computer, before spending macOS CI
# minutes (ADR-0018 section 7, M3 phase 0).
#
#   bash scripts/ios-check.sh
#
# Runs `cargo check --target aarch64-apple-ios` on the crates the phone links,
# without the features that need Apple's SDK or a C toolchain for iOS:
# no `inference` (llama.cpp) and no `stt` (sherpa-onnx's xcframework). Those,
# and the Tauri app itself (its build scripts call Xcode), are checked only by
# the iOS check job on a GitHub macOS runner (.github/workflows/ios.yml).
#
# Installs the Rust target with rustup if it's missing. Works in Git Bash on
# Windows, on Linux and on macOS.
set -euo pipefail

cd "$(dirname "$0")/.."
[ -f scripts/dev-env.sh ] && [ -n "${WINDIR:-}" ] && source scripts/dev-env.sh >/dev/null

TARGET=aarch64-apple-ios
if ! rustup target list --installed | grep -qx "$TARGET"; then
  echo "Installing the Rust target $TARGET"
  rustup target add "$TARGET"
fi

# crate -> features (empty = none). Each is checked on its own, so a feature a
# crate forgets to gate shows up here rather than on the Mac runner.
CHECKS=(
  "os-models|"
  "os-llm|"
  "os-asr|capture"
  "os-diar|"
  "os-engine|recorder"
  "os-app|recorder,sync"
)

failed=0
for entry in "${CHECKS[@]}"; do
  crate="${entry%%|*}"
  features="${entry#*|}"
  args=(check --target "$TARGET" -p "$crate" --no-default-features)
  [ -n "$features" ] && args+=(--features "$features")
  echo "== cargo ${args[*]}"
  if ! RUSTFLAGS="${RUSTFLAGS:-} -D warnings" cargo "${args[@]}"; then
    failed=1
  fi
done

if [ "$failed" -ne 0 ]; then
  echo "ios-check: FAILED" >&2
  exit 1
fi
echo "ios-check: all crates type-check for $TARGET"
