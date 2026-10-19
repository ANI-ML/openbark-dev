#!/usr/bin/env bash
# ADR-0025 end to end on Linux x86_64, with MLX on the CPU ("test mode",
# OPENBARK_MLX_DEV=1): install the MLX engine pack through OpenBark's own
# downloader and check it; add and download a small MLX model through the
# Hugging Face path; draft a note from a fixture transcript with no network
# at all and validate it against the SOAP schema; then remove the model and
# the pack and check the folders are gone and the space freed is right.
#
# Usage: scripts/ci/mlx-e2e.sh path/to/openbark [scratch folder]
#
# Everything goes in the scratch folder (models, engines, visits, keys,
# folder choices): never this computer's own OpenBark folders.
# Optional:
#   MLX_PACK_CACHE=<dir>          keep a copy of the unpacked pack there, keyed
#                                 by its lock id (CI cache); reused when present
#   OPENBARK_GOLDEN_SCHEMAS=<dir> also compile every schema in <dir> in
#                                 llguidance (from `cargo run -p os-engine
#                                 --example write_schemas -- <dir>`)
set -euo pipefail

bin="$(realpath "${1:?usage: mlx-e2e.sh path/to/openbark [scratch]}")"
scratch="${2:-$(mktemp -d -t openbark-mlx-e2e.XXXXXX)}"
mkdir -p "$scratch"
scratch="$(realpath "$scratch")"
repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
model_repo="mlx-community/Qwen3-0.6B-4bit"
model_id="hf:$model_repo/@mlx"
out="$scratch/out"
mkdir -p "$out"

export OPENBARK_MLX_DEV=1
export OPENBARK_MODELS_DIR="$scratch/models"
export OPENBARK_ENGINES_DIR="$scratch/engines"
export OPENBARK_SESSIONS_DIR="$scratch/sessions"
export OPENBARK_TEMPLATES_DIR="$scratch/templates"
export OPENBARK_USERS_FILE="$scratch/users.json"
export OPENBARK_LOCATIONS_FILE="$scratch/locations.json"
export OPENBARK_KEY_FILE="$scratch/data.key"
# A throwaway data key, never this computer's own.
export OPENBARK_DATA_KEY="${OPENBARK_DATA_KEY:-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=}"
export OPENBARK_LOG="${OPENBARK_LOG:-info}"
unset OPENBARK_HF_TOKEN HF_TOKEN

step() { printf '\n=== %s\n' "$*"; }
json() { python3 -c "import json,sys; d=json.load(open(sys.argv[1])); print($2)" "$1"; }
# Bytes in regular files under a folder (links not followed), as OpenBark counts them.
tree_bytes() {
  python3 - "$1" <<'EOF'
import os, sys
total = 0
for root, dirs, files in os.walk(sys.argv[1]):
    for name in files:
        st = os.lstat(os.path.join(root, name))
        if not os.path.islink(os.path.join(root, name)):
            total += st.st_size
print(total)
EOF
}
run_offline() {
  if [[ "$(uname)" == "Linux" ]] && command -v unshare >/dev/null; then
    # -r: map to root inside the namespace, -n: a new network namespace with no interfaces.
    unshare -rn "$@"
  else
    echo "::warning::no network namespace on this OS; running online" >&2
    "$@"
  fi
}

step "MLX status (expect test mode)"
"$bin" mlx status | tee "$out/status-before.json"
[[ "$(json "$out/status-before.json" 'd["support"]')" == "test_mode" ]] || { echo "MLX isn't in test mode"; exit 1; }
lock_id="$(json "$out/status-before.json" 'd["lock_id"]')"
pack_dir="$(json "$out/status-before.json" 'd["pack_dir"]')"

if [[ -n "${MLX_PACK_CACHE:-}" && -f "$MLX_PACK_CACHE/$lock_id/manifest.json" ]]; then
  step "Reusing the cached pack for lock $lock_id"
  mkdir -p "$(dirname "$pack_dir")"
  cp -a "$MLX_PACK_CACHE/$lock_id" "$pack_dir"
fi

step "mlx install (download through OpenBark, unpack, check)"
"$bin" mlx install > "$out/install.json"
cat "$out/install.json"
[[ "$(json "$out/install.json" 'd["ok"]')" == "True" ]]
[[ "$(json "$out/install.json" 'dict(d["checks"])["socket_guard"]')" == "True" ]]

step "mlx check (again)"
"$bin" mlx check > "$out/check.json"
[[ "$(json "$out/check.json" 'd["ok"]')" == "True" ]]
echo "device: $(json "$out/check.json" 'd["device"]'), metal: $(json "$out/check.json" 'd["metal"]')"

step "models add-hf $model_repo @mlx"
added="$("$bin" models add-hf "$model_repo" @mlx | tail -1)"
[[ "$added" == "$model_id" ]] || { echo "added '$added', expected '$model_id'"; exit 1; }

step "models pull $model_id"
"$bin" models pull "$model_id"
"$bin" models verify "$model_id"
model_dir="$OPENBARK_MODELS_DIR/hf_mlx-community/Qwen3-0.6B-4bit/@mlx"
[[ -f "$model_dir/config.json" ]] || { echo "no model folder at $model_dir"; exit 1; }

if [[ -n "${OPENBARK_GOLDEN_SCHEMAS:-}" ]]; then
  step "Golden: every schema compiles in llguidance over the model's tokenizer"
  python_bin="$pack_dir/python/bin/python3"
  env -i HOME="$scratch" HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 "$python_bin" -I - "$model_dir" \
    "$repo_root"/packages/templates/*/schema.json "$OPENBARK_GOLDEN_SCHEMAS"/*.json <<'EOF'
import json, sys
import llguidance, llguidance.hf
from transformers import AutoTokenizer
tok = AutoTokenizer.from_pretrained(sys.argv[1], trust_remote_code=False)
llt = llguidance.hf.from_tokenizer(tok)
bad = []
for p in sys.argv[2:]:
    g = llguidance.LLMatcher.grammar_from_json_schema(json.load(open(p)), defaults={"whitespace_flexible": False})
    is_err, msgs = llguidance.LLMatcher.validate_grammar_with_warnings(g, llt)
    m = llguidance.LLMatcher(llt, g, log_level=0)
    if is_err or m.is_error():
        bad.append([p, msgs, m.get_error()])
print(f"{len(sys.argv) - 2 - len(bad)} of {len(sys.argv) - 2} schemas compile")
if bad:
    print(json.dumps(bad, indent=1))
    sys.exit(1)
EOF
fi

step "Draft a note from the fixture with NO network (unshare -rn)"
fixture="$repo_root/scripts/ci/fixtures/mlx-smoke.jsonl"
run_offline bash -c '
  set -euo pipefail
  if python3 -c "import socket; socket.create_connection((\"huggingface.co\", 443), timeout=3)" 2>/dev/null; then
    echo "network is reachable inside the sandbox" >&2; exit 2
  fi
  "$0" note --input "$1" --llm "$2" --template soap --max-tokens 1024 --json "$3/note.json" > "$3/note.md"
' "$bin" "$fixture" "$model_id" "$out"
echo "--- note ---"
cat "$out/note.md"
python3 "$repo_root/scripts/ci/validate-note.py" "$repo_root/packages/templates/soap/schema.json" "$out/note.json"
python3 - "$out/note.json" <<'EOF'
import json, sys
r = json.load(open(sys.argv[1]))
s = r["stats"]
print(f"model={r['model']} status={r['status']} completion_tokens={s.get('completion_tokens')} total_ms={s.get('total_ms')}")
assert r["status"] == "draft"
EOF

step "models rm $model_id: the files go, the row stays"
expected="$(tree_bytes "$model_dir")"
"$bin" models rm "$model_id" | tail -1 > "$out/rm.json"
cat "$out/rm.json"
freed="$(json "$out/rm.json" 'd["freed_bytes"]')"
[[ "$freed" == "$expected" ]] || { echo "freed $freed bytes, expected $expected"; exit 1; }
[[ ! -e "$model_dir" ]] || { echo "$model_dir is still there"; exit 1; }
[[ ! -e "$OPENBARK_MODELS_DIR/hf_mlx-community" ]] || { echo "empty folders were left behind"; exit 1; }
"$bin" models list | grep -F "$model_id" | grep -q -- ' -$' || { echo "the row should stay, not installed"; exit 1; }

if [[ -n "${MLX_PACK_CACHE:-}" && ! -f "$MLX_PACK_CACHE/$lock_id/manifest.json" ]]; then
  step "Saving the pack to the cache for lock $lock_id"
  mkdir -p "$MLX_PACK_CACHE"
  cp -a "$pack_dir" "$MLX_PACK_CACHE/$lock_id"
fi

step "mlx remove: the pack goes, the space freed matches"
expected="$(tree_bytes "$pack_dir")"
"$bin" mlx remove | tail -1 > "$out/mlx-remove.json"
cat "$out/mlx-remove.json"
freed="$(json "$out/mlx-remove.json" 'd["freed_bytes"]')"
[[ "$freed" == "$expected" ]] || { echo "freed $freed bytes, expected $expected"; exit 1; }
[[ ! -e "$pack_dir" ]] || { echo "$pack_dir is still there"; exit 1; }
"$bin" mlx status > "$out/status-after.json"
[[ "$(json "$out/status-after.json" 'd["installed"]')" == "False" ]]

step "MLX end to end passed (scratch: $scratch)"
