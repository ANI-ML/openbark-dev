#!/usr/bin/env bash
# The desktop app's self-test (apps/desktop/src-tauri/src/selftest.rs), on
# a build with the `selftest` feature: it loads a model, plays two tone
# visits in the app's own webview, then quits the way a person does.
#   scripts/ci/desktop-selftest.sh <openbark-desktop binary> <model.gguf> <work dir>
# On macOS it first quits once without freeing the model (the 0.1.1 crash),
# to show the job can see it. On Linux, run it under xvfb-run.
set -uo pipefail
app="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
model="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
work="$3"
here="$(cd "$(dirname "$0")" && pwd)"
rm -rf "$work"
mkdir -p "$work"
work="$(cd "$work" && pwd)"
summary="${GITHUB_STEP_SUMMARY:-/dev/null}"
fail=0
note() { echo "$*"; echo "$*" >> "$summary"; }
bad() { note "FAIL: $*"; fail=1; }

scratch() {
  local d="$work/$1"
  mkdir -p "$d"
  export OPENBARK_SESSIONS_DIR="$d/visits"
  export OPENBARK_TEMPLATES_DIR="$d/templates"
  export OPENBARK_USERS_FILE="$d/users.json"
  export OPENBARK_LOCATIONS_FILE="$d/locations.json"
  export OPENBARK_ENGINES_DIR="$d/engines"
  export OPENBARK_MODELS_DIR="$d/models"
  export OPENBARK_HF_TOKEN_FILE="$d/hf-token.obk"
  export OPENBARK_DATA_KEY="AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
  export OPENBARK_SELFTEST_OUT="$d/results.json"
  export OPENBARK_LLAMA_LOG=1
}

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
run_app() { run_logged "$1" "$2" "$app"; }

note "### Desktop self-test ($(uname -sm))"

if [ "$(uname -s)" = Darwin ]; then
  scratch control
  export OPENBARK_SELFTEST_LLM="$model" OPENBARK_SELFTEST_SKIP_SHUTDOWN=1
  unset OPENBARK_SELFTEST_AUDIO
  run_app "$work/control.log" 300
  unset OPENBARK_SELFTEST_SKIP_SHUTDOWN
  if grep -q "GGML_ASSERT" "$work/control.log" || [ "$status" = 134 ]; then
    note "- quit without freeing the model (the 0.1.1 behaviour): aborted (status $status), as in the crash report"
    grep -m1 "GGML_ASSERT" "$work/control.log" | sed 's/^/  /' | tee -a "$summary"
  else
    note "- quit without freeing the model: did NOT abort here (status $status); this runner may lack Metal residency sets"
  fi
  grep -m2 -Ei "residency set|GPU name" "$work/control.log" | sed 's/^/  /' | tee -a "$summary"
fi

scratch fixed
export OPENBARK_SELFTEST_LLM="$model" OPENBARK_SELFTEST_AUDIO=1
run_app "$work/fixed.log" 900
if grep -q "GGML_ASSERT" "$work/fixed.log"; then bad "ggml aborted on quit"; grep -A3 GGML_ASSERT "$work/fixed.log" | head; fi
if [ "$status" = 0 ]; then note "- quit with the model loaded: exit 0"; else bad "quit: status $status"; tail -30 "$work/fixed.log"; fi
grep "selftest:" "$work/fixed.log" | sed 's/^/  /' | tee -a "$summary"
if [ -f "$OPENBARK_SELFTEST_OUT" ]; then
  "$(command -v python3 || command -v python)" "$here/check-selftest.py" "$OPENBARK_SELFTEST_OUT" | tee -a "$summary"
  [ "${PIPESTATUS[0]}" = 0 ] || fail=1
else
  bad "no results from the self-test"; tail -30 "$work/fixed.log"
fi
exit "$fail"
