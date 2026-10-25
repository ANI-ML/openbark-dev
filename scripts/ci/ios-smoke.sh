#!/usr/bin/env bash
# The WebView smoke test (ADR-0018 section 8): boot an iPhone simulator,
# install and launch the app with OPENBARK_SMOKE=1, and wait for the marker
# the app writes when the interface has loaded in the WebView and asked the
# engine for its status. Saves a screenshot and the app's log in <out dir>.
#   bash scripts/ci/ios-smoke.sh <path to .app> <out dir>
set -euo pipefail
app="$1"; out="$2"
mkdir -p "$out"
bundle="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")"

# The newest available iPhone simulator.
udid="$(xcrun simctl list devices available -j | python3 -c '
import json, sys
devs = json.load(sys.stdin)["devices"]
best = None
for runtime, items in devs.items():
    if "iOS" not in runtime:
        continue
    for d in items:
        if d.get("isAvailable") and d["name"].startswith("iPhone"):
            key = (runtime, d["name"])
            if best is None or key > best[0]:
                best = (key, d["udid"])
print(best[1] if best else "")
')"
[ -n "$udid" ] || { echo "no iPhone simulator available" >&2; xcrun simctl list devices; exit 1; }
echo "simulator $udid, app $bundle"
xcrun simctl boot "$udid" || true
xcrun simctl bootstatus "$udid" -b
xcrun simctl install "$udid" "$app"

data="$(xcrun simctl get_app_container "$udid" "$bundle" data)"
marker="$data/Library/Application Support/OpenBark/smoke-webview.txt"
rm -f "$marker"

launch() {
  SIMCTL_CHILD_OPENBARK_SMOKE=1 SIMCTL_CHILD_RUST_LOG=info \
    xcrun simctl launch --console-pty --terminate-running-process "$udid" "$bundle" >> "$out/app.log" 2>&1 &
  launcher=$!
}

# Right after a first boot, Springboard can drop the launch without a word
# (no process id, nothing in the log): if nothing started within 30 s, launch
# once more. An app that started and then failed is not retried.
: > "$out/app.log"
launch
ok=0
for i in $(seq 1 120); do
  if [ -f "$marker" ]; then ok=1; break; fi
  if [ "$i" -eq 30 ] && [ ! -s "$out/app.log" ]; then
    echo "the first launch didn't start the app; launching again"
    kill "$launcher" 2>/dev/null || true
    launch
  fi
  sleep 1
done
sleep 3
xcrun simctl io "$udid" screenshot "$out/simulator.png" || true
kill "$launcher" 2>/dev/null || true
xcrun simctl terminate "$udid" "$bundle" || true
# Anything the app left in its container (names only).
find "$data" -maxdepth 5 -print > "$out/container.txt" 2>/dev/null || true

if [ "$ok" -ne 1 ]; then
  echo "The WebView never loaded the interface (no $marker after 120 s)." >&2
  tail -50 "$out/app.log" >&2 || true
  exit 1
fi
echo "WebView smoke test passed: the interface loaded and reached the engine (version $(cat "$marker"))."
