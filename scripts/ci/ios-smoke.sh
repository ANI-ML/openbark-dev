#!/usr/bin/env bash
# The WebView smoke test (ADR-0018 section 8): boot an iPhone simulator,
# install and launch the app with OPENBARK_SMOKE=1, and wait for the marker
# the app writes when the interface has loaded in the WebView and asked the
# engine for its status. Then two checks of the phone model package
# (ADR-0018 section 10):
# - the engine finds the four speech models inside the app, read-only, with
#   nothing downloaded into the app's data container;
# - it transcribes the committed smoke clip (packages/eval/fixtures/audio/
#   smoke.wav, synthetic) with those built-in models, and the transcript isn't
#   empty. The clip is copied into the app's data container for the test
#   only; it never ships in the app;
# - in a build with `inference` (OPENBARK_SMOKE_DRAFT=1, the default), it
#   drafts a SOAP note from that transcript with a small pinned test model
#   (Qwen2.5-0.5B-Instruct Q4_K_M, scripts/ci/fetch-test-gguf.sh: SmolLM2-135M
#   never closes the note), also copied in for the test only, and a note
#   must come back. No LLM ships in the app.
# Saves a screenshot, the app's log and what it reported in <out dir>.
#   bash scripts/ci/ios-smoke.sh <path to .app> <out dir>
set -euo pipefail
app="$1"; out="$2"
mkdir -p "$out"
bundle="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")"
repo="$(cd "$(dirname "$0")/../.." && pwd)"
clip="$repo/packages/eval/fixtures/audio/smoke.wav"
draft="${OPENBARK_SMOKE_DRAFT:-1}"
if [ "$draft" = 1 ]; then
  gguf="$repo/target/test-models/qwen2.5-0.5b-instruct-q4km.gguf"
  bash "$repo/scripts/ci/fetch-test-gguf.sh" "$gguf" qwen2.5-0.5b
fi

# The newest available Face ID iPhone simulator (safe areas at the top and
# bottom show in the screenshot), or the newest iPhone if there is none.
udid="$(xcrun simctl list devices available -j | python3 -c '
import json, sys
devs = json.load(sys.stdin)["devices"]
best = None
for runtime, items in devs.items():
    if "iOS" not in runtime:
        continue
    for d in items:
        if d.get("isAvailable") and d["name"].startswith("iPhone"):
            key = (runtime, "SE" not in d["name"], d["name"])
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
support="$data/Library/Application Support/OpenBark"
marker="$support/smoke-webview.txt"
transcript="$support/smoke-transcript.txt"
note="$support/smoke-note.txt"
rm -f "$marker" "$transcript" "$note"
# The clip the app transcribes under OPENBARK_SMOKE=1 (apps/desktop/src-tauri/src/lib.rs).
mkdir -p "$support"
cp "$clip" "$support/smoke-input.wav"
if [ "$draft" = 1 ]; then cp "$gguf" "$support/smoke-llm.gguf"; fi

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
# With the speech models inside, the app is about 0.7 GB, and a busy runner's
# simulator can take minutes over a first launch.
wait_secs=300
for _ in $(seq 1 $wait_secs); do
  if [ -f "$marker" ]; then ok=1; break; fi
  sleep 1
done
# Then the transcription: loading Parakeet and transcribing the clip on the
# simulator's processor.
heard=0
if [ "$ok" -eq 1 ]; then
  for _ in $(seq 1 300); do
    if [ -f "$transcript" ]; then heard=1; break; fi
    sleep 1
  done
fi
# Then the draft: loading the tiny model and writing a SOAP note on the
# simulator's processor.
drafted=0
if [ "$heard" -eq 1 ] && [ "$draft" = 1 ]; then
  for i in $(seq 1 600); do
    if [ -f "$note" ]; then drafted=1; break; fi
    if [ $((i % 60)) -eq 0 ]; then
      echo "draft after $i s: $(tail -1 "$support/smoke-note.progress" 2>/dev/null || echo 'no progress yet')"
    fi
    sleep 1
  done
fi
sleep 3
xcrun simctl io "$udid" screenshot "$out/simulator.png" || true
# At most a minute (macOS has no timeout command; perl's alarm stands in).
perl -e 'alarm 60; exec @ARGV' xcrun simctl terminate "$udid" "$bundle" 2>/dev/null || true
kill "$logger" 2>/dev/null || true
# Anything the app left in its container (names only).
find "$data" -maxdepth 5 -print > "$out/container.txt" 2>/dev/null || true
cp "$support/smoke-models.json" "$out/models.json" 2>/dev/null || true
cp "$transcript" "$out/transcript.txt" 2>/dev/null || true
cp "$note" "$out/note.txt" 2>/dev/null || true
cp "$support/smoke-note.progress" "$out/note.progress" 2>/dev/null || true

if [ "$ok" -ne 1 ]; then
  # What the simulator said about the app (launch refusals, crashes).
  xcrun simctl spawn "$udid" log show --last 10m --style compact \
    --predicate 'process CONTAINS "OpenBark" OR eventMessage CONTAINS "health.animl.openbark"' \
    > "$out/system.log" 2>&1 || true
  tail -40 "$out/system.log" >&2 || true
  echo "The WebView never loaded the interface (no $marker after $wait_secs s)." >&2
  tail -50 "$out/launch.log" "$out/app.log" >&2 || true
  exit 1
fi
echo "WebView smoke test passed: the interface loaded and reached the engine (version $(cat "$marker"))."

# The speech models come with the app: the engine must find all four in the
# app bundle, read-only, with nothing downloaded into the app's data container.
python3 - "$support/smoke-models.json" "$app" "$data" <<'EOF'
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

# The transcription: the app writes "ok: <first words>" or "error: <why>".
if [ "$heard" -ne 1 ]; then
  echo "The app never wrote $transcript (transcribing the smoke clip with the built-in models)." >&2
  tail -50 "$out/app.log" >&2 || true
  exit 1
fi
result="$(cat "$transcript")"
echo "Transcription on the simulator: $result"
case "$result" in
  ok:\ ?*) echo "Transcription smoke test passed: the built-in models transcribed the clip on the iPhone simulator." ;;
  *) echo "The transcription failed or was empty: $result" >&2; exit 1 ;;
esac

# The draft: the app writes "ok: <n> sections" or "error: <why>".
if [ "$draft" = 1 ]; then
  if [ "$drafted" -ne 1 ]; then
    echo "The app never wrote $note (drafting a note with the test model)." >&2
    echo "Progress:" >&2
    cat "$out/note.progress" >&2 2>/dev/null || echo "none" >&2
    tail -50 "$out/app.log" >&2 || true
    exit 1
  fi
  result="$(cat "$note")"
  echo "Draft on the simulator: $result"
  cat "$out/note.progress" 2>/dev/null || true
  case "$result" in
    ok:\ ?*) echo "Drafting smoke test passed: llama.cpp wrote a note on the iPhone simulator." ;;
    *) echo "The draft failed: $result" >&2; exit 1 ;;
  esac
fi
