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
started_install=$(date +%s)
xcrun simctl install "$udid" "$app"
echo "installed in $(($(date +%s) - started_install)) s ($(du -sk "$app" | cut -f1) KB, the speech models included)"

data="$(xcrun simctl get_app_container "$udid" "$bundle" data)"
marker="$data/Library/Application Support/OpenBark/smoke-webview.txt"
rm -f "$marker"

launch() {
  SIMCTL_CHILD_OPENBARK_SMOKE=1 SIMCTL_CHILD_RUST_LOG=info \
    xcrun simctl launch --console-pty --terminate-running-process "$udid" "$bundle" >> "$out/app.log" 2>&1 &
  launcher=$!
}

# Right after a first boot, Springboard can drop the launch without a word
# (no process id, nothing in the log): if nothing started, launch again, at
# 60 s and 150 s. With the speech models inside, the app is about 0.7 GB, and
# a busy runner's simulator can take minutes over a first launch. An app
# that started and then failed is not retried.
: > "$out/app.log"
launch
ok=0
wait_secs=300
for i in $(seq 1 $wait_secs); do
  if [ -f "$marker" ]; then ok=1; break; fi
  if { [ "$i" -eq 60 ] || [ "$i" -eq 150 ]; } && [ ! -s "$out/app.log" ]; then
    echo "no sign of the app after $i s; launching again"
    kill "$launcher" 2>/dev/null || true
    launch
  fi
  sleep 1
done
sleep 3
xcrun simctl io "$udid" screenshot "$out/simulator.png" || true
kill "$launcher" 2>/dev/null || true
# At most a minute (macOS has no timeout command; perl's alarm stands in).
perl -e 'alarm 60; exec @ARGV' xcrun simctl terminate "$udid" "$bundle" 2>/dev/null || true
# Anything the app left in its container (names only).
find "$data" -maxdepth 5 -print > "$out/container.txt" 2>/dev/null || true

if [ "$ok" -ne 1 ]; then
  # What the simulator said about the app (launch refusals, crashes).
  xcrun simctl spawn "$udid" log show --last 10m --style compact     --predicate 'process CONTAINS "OpenBark" OR eventMessage CONTAINS "health.animl.openbark"'     > "$out/system.log" 2>&1 || true
  tail -40 "$out/system.log" >&2 || true
  echo "The WebView never loaded the interface (no $marker after $wait_secs s)." >&2
  tail -50 "$out/app.log" >&2 || true
  exit 1
fi
echo "WebView smoke test passed: the interface loaded and reached the engine (version $(cat "$marker"))."

# The speech models come with the app (ADR-0018 section 10): the engine must
# find all four in the app bundle, read-only, with nothing downloaded into the
# app's data container.
models_json="$data/Library/Application Support/OpenBark/smoke-models.json"
cp "$models_json" "$out/models.json" 2>/dev/null || true
python3 - "$models_json" "$app" "$data" <<'EOF'
import json, os, sys
want = ["silero-vad", "parakeet-tdt-0.6b-v3-int8", "pyannote-segmentation-3.0", "wespeaker-en-resnet34-lm"]
status_file, app, data = sys.argv[1:4]
try:
    models = {m["id"]: m for m in json.load(open(status_file))}
except (OSError, ValueError) as e:
    sys.exit(f"the app didn't report its models ({e})")
problems = []
for mid in want:
    m = models.get(mid)
    if not m:
        problems.append(f"{mid}: not reported")
    elif not (m.get("installed") and m.get("bundled")):
        problems.append(f"{mid}: installed={m.get('installed')} bundled={m.get('bundled')}")
    if not os.path.isdir(os.path.join(app, "assets", "models", mid)):
        problems.append(f"{mid}: not in {app}/assets/models")
downloaded = [os.path.join(d, f) for d, _, files in os.walk(data) for f in files if any(w in d for w in want)]
if downloaded:
    problems.append("downloaded into the data container: " + ", ".join(downloaded[:5]))
if problems:
    sys.exit("Bundled speech models: " + "; ".join(problems))
print("Bundled speech models: all four found in the app, read-only; nothing downloaded.")
EOF
