#!/usr/bin/env bash
# Benchmark the full pipeline on this machine and save the results under
# docs/perf/results/<host>-<date>/ (acceptance test M0-3).
#
# Usage:  scripts/bench.sh [llm-id ...]      (default: gemma-4-e4b-it-q4_0 gemma-4-12b-it-q4_0)
# Needs:  target/release/openbark (built with --features metal on Mac, cuda on NVIDIA)
#         the paced fixture audio (downloaded automatically from the fixtures release)
set -euo pipefail
cd "$(dirname "$0")/.."

bin=target/release/openbark
[[ -x "$bin" || -x "$bin.exe" ]] || { echo "build first: cargo build --release -p os-cli --features metal|cuda"; exit 1; }
llms=("${@:-gemma-4-e4b-it-q4_0}")
[[ $# -eq 0 ]] && llms=(gemma-4-e4b-it-q4_0 gemma-4-12b-it-q4_0)

audio=packages/eval/fixtures/audio
fixtures=(feline-vomiting-paced feline-vomiting)
for f in "${fixtures[@]}"; do
  if [[ ! -f "$audio/$f.wav" ]]; then
    echo "fetching fixture audio from the 'fixtures-v1' release…"
    gh release download fixtures-v1 --pattern '*.wav' --dir "$audio" --skip-existing
    break
  fi
done

for id in silero-vad parakeet-tdt-0.6b-v3-int8 pyannote-segmentation-3.0 wespeaker-en-resnet34-lm "${llms[@]}"; do
  "$bin" models pull "$id"
done

host=$(hostname | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9-\n' '-')
out="docs/perf/results/${host}-$(date +%Y%m%d)"
mkdir -p "$out"
"$bin" probe > "$out/probe.json"

for llm in "${llms[@]}"; do
  for f in "${fixtures[@]}"; do
    echo "== $f × $llm"
    # Warm run discarded (first model load hits a cold disk cache), then measured run.
    "$bin" bench "$audio/$f.wav" --diarize --llm "$llm" > /dev/null
    "$bin" bench "$audio/$f.wav" --diarize --llm "$llm" --markdown "$out/$f.$llm.md" > "$out/$f.$llm.json"
    python3 - "$out/$f.$llm.json" <<'EOF' || python - "$out/$f.$llm.json" <<'EOF2'
import json, sys
r = json.load(open(sys.argv[1]))
t, n = r["transcribe"], r["note"]
print(f"  audio {r['audio_secs']/60:.1f} min | STT+diar {t['total_ms']/1000:.1f}s (RTF {t['rtf']:.3f}) | "
      f"LLM load {r['llm_load_ms']/1000:.1f}s | TTFT {n['time_to_first_token_ms']/1000:.1f}s | "
      f"{n['tokens_per_second']:.0f} tok/s | wall {r['wall_ms']/1000:.1f}s | faithful={r['faithfulness']['passed']}")
EOF
import json, sys
r = json.load(open(sys.argv[1]))
print(f"  wall {r['wall_ms']/1000:.1f}s faithful={r['faithfulness']['passed']}")
EOF2
  done
done
echo "results in $out. Commit them on a branch, or paste them into the chat."
