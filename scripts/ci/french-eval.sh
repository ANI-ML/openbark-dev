#!/usr/bin/env bash
# The French eval's recogniser runs (.github/workflows/french-eval.yml,
# docs/eval/french.md). Each recogniser transcribes every fixture alone, with
# `openbark transcribe` (voice detection and one recogniser, no speaker labels
# and no second listen), on the threads OPENBARK_ASR_THREADS gives, one after
# another on the same runner so the speeds compare.
#
#   french-eval.sh asr OPENBARK SETS_DIR OUT_DIR
#
# SETS_DIR holds fr/audio and en/audio (rendered fixtures). Writes
# OUT_DIR/<set>/<arm>/<fixture>.jsonl (the transcript) and <fixture>.log (its
# standard error; the last JSON line holds the timings).
set -euo pipefail

# arm=recogniser:language ("" lets it detect the language; Parakeet takes none).
FR_ARMS=(
  "parakeet=parakeet-tdt-0.6b-v3-int8:"
  "whisper-fr=whisper-large-v3-turbo-int8:fr"
)
EN_ARMS=(
  "parakeet=parakeet-tdt-0.6b-v3-int8:"
  "whisper-en=whisper-large-v3-turbo-int8:en"
)

asr() {
  local bin=$1 sets=$2 out=$3
  for set in fr en; do
    local arms
    if [ "$set" = fr ]; then arms=("${FR_ARMS[@]}"); else arms=("${EN_ARMS[@]}"); fi
    for spec in "${arms[@]}"; do
      local arm=${spec%%=*} rest=${spec#*=}
      local stt=${rest%%:*} lang=${rest#*:}
      local dir="$out/$set/$arm"
      mkdir -p "$dir"
      for wav in "$sets/$set/audio"/*.wav; do
        local f
        f=$(basename "$wav" .wav)
        local extra=()
        [ -n "$lang" ] && extra=(--language "$lang")
        if ! "$bin" transcribe "$wav" --stt "$stt" "${extra[@]}" > "$dir/$f.jsonl" 2> "$dir/$f.log"; then
          # One fixture failing leaves the others' numbers.
          echo "::warning::$set $arm $f failed"
          tail -5 "$dir/$f.log"
          rm -f "$dir/$f.jsonl"
          continue
        fi
        echo "$set $arm $f: $(wc -l < "$dir/$f.jsonl") segments; $(tail -1 "$dir/$f.log")"
      done
    done
  done
}

cmd=${1:?asr}
shift
case "$cmd" in
  asr) asr "$@" ;;
  *) echo "unknown command $cmd" >&2; exit 2 ;;
esac
