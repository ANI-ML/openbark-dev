#!/usr/bin/env bash
# Acceptance tests M0-2, M0-4 and M0-6: the full pipeline runs with NO network
# (Linux network namespace) and produces a schema-valid note whose numbers are
# all supported by the transcript.
# Usage: e2e-offline.sh path/to/openbark <fixture-id> <llm-id>
set -euo pipefail
bin="$(realpath "${1:?binary}")"
fixture="${2:-smoke}"
llm="${3:-gemma-4-e2b-it-q4_0}"
wav="packages/eval/fixtures/audio/$fixture.wav"
out="target/e2e"
mkdir -p "$out"

run_offline() {
  if [[ "$(uname)" == "Linux" ]] && command -v unshare >/dev/null; then
    # -r: map to root inside the namespace, -n: new network namespace with no interfaces.
    unshare -rn "$@"
  else
    echo "::warning::no network namespace on this OS; running online" >&2
    "$@"
  fi
}

run_offline bash -c "
  set -euo pipefail
  if curl -s --max-time 3 https://huggingface.co >/dev/null 2>&1; then echo 'network is reachable inside the sandbox' >&2; exit 2; fi
  '$bin' transcribe '$wav' --diarize > '$out/$fixture.jsonl'
  '$bin' note --input '$out/$fixture.jsonl' --llm '$llm' --gpu-layers \${OPENBARK_GPU_LAYERS:-0} --json '$out/$fixture.note.json' > '$out/$fixture.md'
"

python3 - "$out/$fixture.note.json" <<'EOF'
import json, sys
result = json.load(open(sys.argv[1]))
f = result["faithfulness"]
print(f"status={result['status']} statements={f['statements_checked']} numbers={f['numbers_checked']} passed={f['passed']}")
assert result["status"] == "draft"
if not f["passed"]:
    print("unsupported numbers:", f["unsupported_numbers"])
    print("invalid evidence:", f["invalid_evidence"])
    sys.exit("faithfulness check failed (hard fail: invented numbers)")
EOF
echo "--- note ---"
cat "$out/$fixture.md"
