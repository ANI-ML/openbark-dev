#!/usr/bin/env bash
# Native microphone capture on a macOS runner (CoreAudio through cpal), with
# virtual microphones from BlackHole: no microphone at all, then recording
# from one, the system default switched to another mid-recording (as when
# AirPods connect), and the recording's transcript afterwards.
#
#   bash scripts/ci/mic-macos.sh <openbark binary> <models dir> <out dir>
#
# The models dir needs silero-vad and parakeet-tdt-0.6b-v3-int8. Speech is
# made with `say` and played into the virtual device, so the "microphone"
# hears it. If macOS hands the process silence (no microphone permission
# for the runner's shell), the recorder must say so; that's reported, not a
# failure of this check.
set -uo pipefail
bin="$1"; models="$2"; out="$3"
mkdir -p "$out"
export OPENBARK_SESSIONS_DIR="$out/visits" OPENBARK_USERS_FILE="$out/users.json" \
  OPENBARK_LOCATIONS_FILE="$out/locations.json" OPENBARK_KEY_FILE="$out/data.key" \
  OPENBARK_DATA_KEY="AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=" OPENBARK_LOG="os_asr=info,os_engine=info"
fail=0
note() { echo "$*" | tee -a "$out/summary.md"; }

note "## Native microphone on macOS"
note ""
note "### 1. Devices before BlackHole"
"$bin" devices > "$out/devices-before.txt" 2>&1
note '```'; cat "$out/devices-before.txt" | tee -a "$out/summary.md"; note '```'
if ! grep -q . "$out/devices-before.txt" || grep -qi "no microphones" "$out/devices-before.txt"; then
  # No input at all: the recorder must refuse with the plain sentence.
  printf '\n' | "$bin" record --no-note --no-speakers --models-dir "$models" > "$out/no-device.txt" 2>&1
  if grep -q "No microphone was found" "$out/no-device.txt"; then
    note "- ok: with no microphone, recording refuses with the plain message"
  else
    note "- note: no-microphone run said: $(tail -3 "$out/no-device.txt" | tr '\n' ' ')"
  fi
fi

note ""
note "### 2. Two virtual microphones (BlackHole 2ch and 16ch)"
brew install --quiet blackhole-2ch blackhole-16ch switchaudio-osx > "$out/brew.log" 2>&1 || true
sudo killall coreaudiod > /dev/null 2>&1 || true
for _ in $(seq 1 20); do SwitchAudioSource -a -t input 2> /dev/null | grep -q "BlackHole 16ch" && break; sleep 1; done
SwitchAudioSource -a -t input > "$out/inputs.txt" 2>&1
note '```'; cat "$out/inputs.txt" | tee -a "$out/summary.md"; note '```'
if ! grep -q "BlackHole 2ch" "$out/inputs.txt" || ! grep -q "BlackHole 16ch" "$out/inputs.txt"; then
  note "- FAIL: BlackHole devices didn't appear; can't test capture on this runner"
  exit 1
fi
SwitchAudioSource -t input -s "BlackHole 2ch" > /dev/null
SwitchAudioSource -t output -s "BlackHole 2ch" > /dev/null
say -o "$out/part1.aiff" "The dog has been vomiting for two days. Her temperature is thirty nine point two degrees."
say -o "$out/part2.aiff" "We will start maropitant at one milligram per kilogram once daily for four days."

mkfifo "$out/ctl"
"$bin" record --no-note --no-speakers --models-dir "$models" < "$out/ctl" > "$out/transcript.txt" 2> "$out/record.log" &
pid=$!
exec 3> "$out/ctl"
# Wait for the recorder to start (models load first).
for _ in $(seq 1 120); do grep -q "Recording. Press Enter" "$out/record.log" && break; sleep 0.5; done
sleep 1
afplay "$out/part1.aiff"
sleep 1
# The default microphone changes mid-recording (AirPods connecting).
SwitchAudioSource -t input -s "BlackHole 16ch" > /dev/null
SwitchAudioSource -t output -s "BlackHole 16ch" > /dev/null
sleep 4
afplay "$out/part2.aiff"
sleep 2
echo >&3
exec 3>&-
wait "$pid"
code=$?
tr '\r' '\n' < "$out/record.log" | grep -v "REC " > "$out/record-clean.log"
note ""
note "### 3. Recording with a switch of default microphone"
note '```'; tail -25 "$out/record-clean.log" | tee -a "$out/summary.md"; note '```'
note '```'; cat "$out/transcript.txt" | tee -a "$out/summary.md"; note '```'
[ "$code" = 0 ] || { note "- FAIL: the recorder exited with $code"; fail=1; }
if grep -q "no sound from the microphone" "$out/record-clean.log"; then
  note "- finding: macOS gave the runner's process silence (microphone permission); the recorder said so"
else
  grep -q "now recording from BlackHole 16ch" "$out/record-clean.log" \
    && note "- ok: the recording followed the new default microphone" \
    || { note "- FAIL: no switch to BlackHole 16ch reported"; fail=1; }
  segments=$(grep -c '^\[' "$out/transcript.txt" || true)
  [ "$segments" -ge 2 ] \
    && note "- ok: $segments segments transcribed, before and after the switch" \
    || { note "- FAIL: only $segments segments transcribed"; fail=1; }
fi
SwitchAudioSource -t input -s "BlackHole 2ch" > /dev/null 2>&1 || true
exit $fail
