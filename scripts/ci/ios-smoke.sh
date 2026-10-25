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

# The app's own log, streamed from the simulator (no terminal needed).
xcrun simctl spawn "$udid" log stream --style compact --level info \
  --predicate 'subsystem BEGINSWITH "health.animl.openbark" OR process == "OpenBark"' > "$out/app.log" 2>&1 &
logger=$!

running() {
  xcrun simctl spawn "$udid" launchctl list 2>/dev/null | grep -q "$bundle"
}

# Launch without a console: on CI runners there's no terminal, and right after
# a first boot Springboard can drop a launch without a word. Check the app is
# really running and launch again (up to three times) if it isn't.
ok=0
for attempt in 1 2 3; do
  if SIMCTL_CHILD_OPENBARK_SMOKE=1 SIMCTL_CHILD_RUST_LOG=info \
    xcrun simctl launch --terminate-running-process "$udid" "$bundle" >> "$out/launch.log" 2>&1; then
    sleep 5
    running && break
  fi
  echo "launch $attempt didn't start the app; trying again"
  sleep 10
done
for _ in $(seq 1 120); do
  if [ -f "$marker" ]; then ok=1; break; fi
  sleep 1
done
sleep 3
xcrun simctl io "$udid" screenshot "$out/simulator.png" || true
xcrun simctl terminate "$udid" "$bundle" || true
kill "$logger" 2>/dev/null || true
# Anything the app left in its container (names only).
find "$data" -maxdepth 5 -print > "$out/container.txt" 2>/dev/null || true

if [ "$ok" -ne 1 ]; then
  echo "The WebView never loaded the interface (no $marker after 120 s)." >&2
  tail -50 "$out/launch.log" "$out/app.log" >&2 || true
  exit 1
fi
echo "WebView smoke test passed: the interface loaded and reached the engine (version $(cat "$marker"))."
