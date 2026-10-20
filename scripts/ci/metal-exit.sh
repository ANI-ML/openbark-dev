#!/usr/bin/env bash
# The macOS quit crash of 0.1.1 (SIGABRT in ggml_metal_rsets_free at exit),
# for the engine: a real model on Metal, then each way the process ends.
#   scripts/ci/metal-exit.sh <openbark> <exit_order example> <model.gguf> <work dir>
#
# - exit_order leak: the old behaviour (model still loaded at exit). On a Mac
#   with Metal residency sets this aborts; that's how we know the job can see
#   the crash. If it doesn't abort here, the summary says so.
# - exit_order app / busy: the model freed first (idle, and mid-draft): exit 0.
# - openbark note: a normal CLI run: exit 0. Ctrl+C mid-run: ends by the
#   signal, with no abort.
# - openbark serve: a model loaded through the API, then Ctrl+C, then
#   SIGTERM: exit 0 each time, no abort.
# Every run is checked for "GGML_ASSERT" and for a new crash report.
set -uo pipefail
# Job control: without it, bash starts background commands with SIGINT
# ignored, so the Ctrl+C checks below would test nothing (and hang).
set -m
bin="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
example="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
model="$(cd "$(dirname "$3")" && pwd)/$(basename "$3")"
work="$4"
rm -rf "$work"
mkdir -p "$work"
work="$(cd "$work" && pwd)"
summary="${GITHUB_STEP_SUMMARY:-/dev/null}"
fail=0

# Scratch everything: never this computer's own data.
export OPENBARK_SESSIONS_DIR="$work/visits"
export OPENBARK_TEMPLATES_DIR="$work/templates"
export OPENBARK_USERS_FILE="$work/users.json"
export OPENBARK_LOCATIONS_FILE="$work/locations.json"
export OPENBARK_ENGINES_DIR="$work/engines"
export OPENBARK_MODELS_DIR="$work/models"
export OPENBARK_HF_TOKEN_FILE="$work/hf-token.obk"
export OPENBARK_DATA_KEY="AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
export OPENBARK_LLAMA_LOG=1 # ggml's own log, so the Metal device and residency sets show

reports_dir="$HOME/Library/Logs/DiagnosticReports"
reports() { ls "$reports_dir" 2>/dev/null | grep -Ei 'openbark|exit_order' | sort; }
before_reports="$(reports)"

note() { echo "$*"; echo "$*" >> "$summary"; }
bad() { note "FAIL: $*"; fail=1; }

# Run "$@" with a time limit in seconds; sets $status. Polls rather than
# starting a watchdog, so no stray process keeps the step's output open.
run_logged() {
  local log="$1" limit="$2"; shift 2
  "$@" > "$log" 2>&1 &
  local pid=$! ticks=0
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$ticks" -ge $((limit * 10)) ]; then echo "timed out after ${limit}s" >> "$log"; kill -KILL "$pid" 2>/dev/null; break; fi
    sleep 0.1; ticks=$((ticks + 1))
  done
  wait "$pid"; status=$?
}

# Wait for $1 for up to $2 seconds, then kill it; sets $status.
wait_bounded() {
  local pid="$1" limit="$2" ticks=0
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$ticks" -ge $((limit * 10)) ]; then echo "  (still running after ${limit}s: killed)"; kill -KILL "$pid" 2>/dev/null; break; fi
    sleep 0.1; ticks=$((ticks + 1))
  done
  wait "$pid"; status=$?
}

no_abort() {
  if grep -q "GGML_ASSERT" "$1"; then bad "$2: ggml aborted"; grep -A3 "GGML_ASSERT" "$1" | head -8; fi
}

note "### Quit with a model loaded ($(uname -sm))"

# 1. Negative control: the 0.1.1 behaviour.
run_logged "$work/leak.log" 300 "$example" "$work/leak" "$model" leak
if grep -q "GGML_ASSERT(\[rsets->data count\] == 0)" "$work/leak.log" || [ "$status" = 134 ]; then
  note "- leak (model kept at exit, the old behaviour): aborted as in the crash report (status $status). The job reproduces the bug."
  grep -m1 "GGML_ASSERT" "$work/leak.log" | sed 's/^/  /' | tee -a "$summary"
  echo reproduced > "$work/leak.result"
else
  note "- leak (model kept at exit): did NOT abort here (status $status); this runner may lack Metal residency sets, so the checks below can't prove the fix on it."
  grep -m3 -Ei "residency|MTL|metal" "$work/leak.log" | sed 's/^/  /' | tee -a "$summary"
  echo not-reproduced > "$work/leak.result"
fi
grep -m2 -Ei "residency set|GPU name" "$work/leak.log" | sed 's/^/  /' | tee -a "$summary"

# 2. The fix, idle and mid-draft.
for mode in app busy; do
  run_logged "$work/$mode.log" 300 "$example" "$work/$mode" "$model" "$mode"
  no_abort "$work/$mode.log" "exit_order $mode"
  if [ "$status" = 0 ]; then note "- exit_order $mode: exit 0"; else bad "exit_order $mode: status $status"; tail -20 "$work/$mode.log"; fi
  grep "exit_order:" "$work/$mode.log" | sed 's/^/  /' | tee -a "$summary"
done

# 3. The CLI: a normal run, and Ctrl+C mid-run.
fixture="$(cd "$(dirname "$0")" && pwd)/fixtures/mlx-smoke.jsonl"
# A short draft: GitHub's Mac runners are slow on their paravirtual GPU, and
# a tiny model's truncated note may be refused (exit 1). What matters is how
# the process ends: by itself, without an abort or a signal.
run_logged "$work/note.log" 600 "$bin" note --input "$fixture" --llm "$model" --template soap --max-tokens 48
no_abort "$work/note.log" "openbark note"
if [ "$status" -lt 128 ]; then note "- openbark note: ended by itself (status $status)"; else bad "openbark note: status $status"; tail -20 "$work/note.log"; fi

"$bin" note --input "$fixture" --llm "$model" --template soap --max-tokens 4000 > "$work/note-int.log" 2>&1 &
pid=$!
for _ in $(seq 1 600); do grep -q "llm loaded\|load_tensors" "$work/note-int.log" 2>/dev/null && break; sleep 0.1; done
sleep 1
kill -INT "$pid" 2>/dev/null
wait_bounded "$pid" 60
no_abort "$work/note-int.log" "openbark note, Ctrl+C"
if [ "$status" = 130 ]; then
  note "- openbark note, Ctrl+C mid-run: ended by the signal (status 130), no abort"
else
  bad "openbark note, Ctrl+C mid-run: status $status (expected 130)"; tail -5 "$work/note-int.log"
fi

# 4. Web mode: a model loaded through the API, then Ctrl+C; then SIGTERM.
mkdir -p "$work/ui"
echo '<!doctype html><title>OpenBark</title>' > "$work/ui/index.html"
for sig in INT TERM; do
  "$bin" serve --port 4338 --ui-dir "$work/ui" > "$work/serve-$sig.log" 2>&1 &
  pid=$!
  for _ in $(seq 1 100); do curl -fs http://127.0.0.1:4338/api/status > /dev/null 2>&1 && break; sleep 0.2; done
  body=$(printf '{"llm":"%s","text":"Subjective: vomiting two days. Plan: bland diet."}' "$model")
  # Loads the model, then drafts; on a slow GPU the draft is still running
  # when the signal comes (the "quit mid-draft" case).
  curl -s --max-time 60 -o "$work/from-example-$sig.json" -w "%{http_code}\n" -H "x-openbark: 1" -H "content-type: application/json" \
    -d "$body" http://127.0.0.1:4338/api/templates/from-example > "$work/from-example-$sig.code" || true
  kill "-$sig" "$pid" 2>/dev/null
  wait_bounded "$pid" 60
  no_abort "$work/serve-$sig.log" "openbark serve, SIG$sig"
  if [ "$status" = 0 ] && grep -q "OpenBark is stopping." "$work/serve-$sig.log"; then
    note "- openbark serve, SIG$sig after loading the model (HTTP $(cat "$work/from-example-$sig.code")): exit 0"
  else
    bad "openbark serve, SIG$sig: status $status"; tail -20 "$work/serve-$sig.log"
  fi
done

# 5. No new crash reports from the runs that must exit cleanly.
sleep 2
new_reports="$(comm -13 <(echo "$before_reports") <(reports))"
if [ -n "$new_reports" ]; then
  note "- crash reports written during the runs:"
  echo "$new_reports" | sed 's/^/  /' | tee -a "$summary"
  mkdir -p "$work/crash-reports"
  echo "$new_reports" | while read -r f; do [ -n "$f" ] && cp "$reports_dir/$f" "$work/crash-reports/"; done
  # The leak run is expected to leave one; any other is a failure.
  if echo "$new_reports" | grep -v -i "exit_order" | grep -q .; then bad "a crash report from openbark"; fi
  count=$(echo "$new_reports" | grep -ci "exit_order" || true)
  [ "$(cat "$work/leak.result")" = reproduced ] && [ "$count" -gt 1 ] && bad "more than one exit_order crash report (only leak may crash)"
fi

[ "$fail" = 0 ] && note "All clean exits were clean." || note "Some runs failed."
exit "$fail"
