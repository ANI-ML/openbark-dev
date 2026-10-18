#!/usr/bin/env bash
# Spike S2 / acceptance test M0-5: the binary must contain exactly one copy of
# ggml (from llama.cpp), no whisper.cpp, and one ONNX Runtime.
# Usage: check-symbols.sh path/to/openbark   (Linux or macOS binary)
set -euo pipefail
bin="${1:?binary path}"
syms=$(nm -g --defined-only "$bin" 2>/dev/null || nm -gU "$bin")

count() { grep -cE "$1" <<<"$syms" || true; }

ggml=$(count '[[:space:]]_?ggml_init$')
whisper=$(count '[[:space:]]_?whisper_init')
ort=$(count '[[:space:]]_?OrtGetApiBase$')

echo "ggml_init definitions: $ggml   whisper_init*: $whisper   OrtGetApiBase: $ort"
fail=0
[[ "$ggml" -eq 1 ]] || { echo "::error::expected exactly one ggml (got $ggml)"; fail=1; }
[[ "$whisper" -eq 0 ]] || { echo "::error::whisper.cpp must not be linked (ADR-0001)"; fail=1; }
# ORT is statically linked on macOS (one copy expected); on Windows and Linux it
# is a shared library beside the binary (ADR-0001, amendment of 2026-10-18).
[[ "$ort" -le 1 ]] || { echo "::error::more than one ONNX Runtime ($ort)"; fail=1; }
if [[ "$(uname -s)" == Linux ]]; then
  # The binary must find sherpa-onnx and ONNX Runtime beside itself ($ORIGIN),
  # with no LD_LIBRARY_PATH, and load exactly one libonnxruntime.
  dyn=$(readelf -d "$bin")
  grep -qE 'R(UN)?PATH.*\$ORIGIN' <<<"$dyn" || { echo "::error::no \$ORIGIN run path in $bin"; fail=1; }
  libs=$(env -u LD_LIBRARY_PATH ldd "$bin")
  missing=$(grep 'not found' <<<"$libs" || true)
  [[ -z "$missing" ]] || { echo "::error::libraries not found: $missing"; fail=1; }
  ort_so=$(grep -c 'libonnxruntime' <<<"$libs" || true)
  echo "libonnxruntime loaded: $ort_so"
  [[ "$ort" -eq 0 && "$ort_so" -eq 1 ]] || { echo "::error::expected one shared ONNX Runtime (static $ort, shared $ort_so)"; fail=1; }
fi
exit $fail
