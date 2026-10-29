#!/usr/bin/env bash
# The speaker labels eval (.github/workflows/speakers-eval.yml). Renders
# harder synthetic consults than the M1 fixtures with Kokoro, imports each
# through the live pipeline the app uses (`openbark record --file`, as an
# import runs), and keeps the saved transcripts for packages/eval/speaker_suite.py.
#
#   speaker-eval.sh render FIXTURE_TTS KOKORO_DIR OUT_DIR [CACHE_DIR]
#   speaker-eval.sh run OPENBARK AUDIO_DIR OUT_DIR NAME [record options...]
#
# `run` writes OUT_DIR/NAME/<fixture>.jsonl (the saved transcript),
# OUT_DIR/NAME-live/<fixture>.jsonl (segments as they were heard, with the
# labels shown while recording) and OUT_DIR/NAME/<fixture>.secs (how long
# the import took). Fixtures ending in -phone run with --speakerphone.
set -euo pipefail
scripts=packages/eval/fixtures/scripts

render() {
  local tts=$1 kokoro=$2 out=$3 cache=${4:-}
  local c=()
  [ -n "$cache" ] && c=(--cache "$cache")
  voice() { "$tts" --kokoro "$kokoro" --out "$out" "${c[@]}" "$@"; }
  local s=$scripts
  mkdir -p "$out"
  # Turns 0.1 s apart: people answering each other quickly.
  voice --pause 0.1 --suffix -tight \
    $s/feline-vomiting.yaml $s/multipet.yaml $s/emergency.yaml $s/production-animal.yaml
  # The same, heard by a laptop's own microphone across the room (reverb and
  # noise), plus a one-speaker dictation.
  voice --pause 0.1 --room 3 --suffix -room \
    $s/feline-vomiting.yaml $s/multipet.yaml $s/emergency.yaml $s/production-animal.yaml \
    $s/canine-wellness.yaml $s/dental-young-dog.yaml
  # Two similar voices in the room: both British women, both American men.
  voice --pause 0.1 --room 3 --voice client=bf_isabella --suffix -same $s/canine-wellness.yaml
  voice --pause 0.1 --room 3 --voice client=am_adam --suffix -same $s/feline-vomiting.yaml
  # The scripts' own pauses, no room: the conditions the labels were tuned in.
  voice --suffix -orig $s/feline-vomiting.yaml $s/canine-wellness.yaml
  # A client on a phone on speaker (ADR-0026).
  voice --suffix -phone $s/speaker-diabetic-callback.yaml $s/speaker-dropoff-lameness.yaml
}

run() {
  local bin=$1 audio=$2 out=$3 name=$4
  shift 4
  mkdir -p "$out/$name" "$out/$name-live"
  for wav in "$audio"/*.wav; do
    local f
    f=$(basename "$wav" .wav)
    local extra=()
    [[ $f == *-phone ]] && extra=(--speakerphone)
    local visits
    visits=$(mktemp -d)
    local t0
    t0=$(date +%s.%N)
    OPENBARK_SESSIONS_DIR="$visits" "$bin" record --file "$wav" --no-note --jsonl \
      --transcript-out "$out/$name/$f.jsonl" "${extra[@]}" "$@" \
      > "$out/$name-live/$f.jsonl" 2>> "$out/$name/log.txt"
    awk -v a="$t0" -v b="$(date +%s.%N)" 'BEGIN { printf "%.1f\n", b - a }' > "$out/$name/$f.secs"
    rm -rf "$visits"
    echo "$name $f: $(wc -l < "$out/$name/$f.jsonl") segments, $(cat "$out/$name/$f.secs") s"
  done
}

cmd=${1:?render or run}
shift
case "$cmd" in
  render) render "$@" ;;
  run) run "$@" ;;
  *) echo "unknown command $cmd" >&2; exit 2 ;;
esac
