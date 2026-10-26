#!/usr/bin/env bash
# Get the Xcode project ready for an iPhone build that transcribes (ADR-0018
# sections 1 and 10). macOS with Xcode only; used by the iOS check
# (ios-sim-build.sh) and the TestFlight job.
# - sherpa-onnx's iOS library: the official v1.13.8 xcframework, the crate's
#   own version, pinned by SHA-256 (scripts/ci/ios-sherpa.py), unpacked into
#   gen/apple/Frameworks, and its archive left where the crate's build script
#   reads it, so the build downloads nothing itself;
# - the Xcode project regenerated from project.yml (XcodeGen), which links and
#   embeds that framework.
# Writes SHERPA_ONNX_ARCHIVE_DIR=<dir> to $GITHUB_ENV when set, and prints it
# on its last line.
#   bash scripts/ci/ios-prepare.sh
set -euo pipefail
cd "$(dirname "$0")/../.."
archive_dir="$(python3 scripts/ci/ios-sherpa.py | tail -1)"
# The app's version comes from tauri.conf.json (the App Store needs it to rise
# with each release); the build number is the TestFlight job's --build-number.
python3 - <<'PY'
import json, re
conf = "apps/desktop/src-tauri/tauri.conf.json"
spec = "apps/desktop/src-tauri/gen/apple/project.yml"
version = json.load(open(conf, encoding="utf-8"))["version"]
s = open(spec, encoding="utf-8").read()
s, n = re.subn(r"(CFBundleShortVersionString: )\S+", r"\g<1>" + version, s)
s = re.sub(r'(CFBundleVersion: )"[^"]*"', r'\g<1>"' + version + '"', s)
assert n == 1, "CFBundleShortVersionString not found in project.yml"
open(spec, "w", encoding="utf-8", newline="\n").write(s)
print("iOS version", version)
PY
command -v xcodegen >/dev/null || brew install xcodegen
# Externals (the Rust library) and assets (bundle > resources) are filled in by
# the Tauri CLI during the build; XcodeGen only needs them to exist.
(cd apps/desktop/src-tauri/gen/apple && mkdir -p Externals assets && xcodegen generate --spec project.yml)
[ -n "${GITHUB_ENV:-}" ] && echo "SHERPA_ONNX_ARCHIVE_DIR=$archive_dir" >> "$GITHUB_ENV"
echo "$archive_dir"
