#!/usr/bin/env bash
# Local-first guard (docs/network.md): the only engine crate allowed an HTTP
# client is os-models. Fails if any other workspace crate pulls in reqwest,
# ureq, hyper-based clients or similar.
set -euo pipefail

allowed="os-models"
violations=0
for client in reqwest ureq isahc attohttpc surf; do
  # Workspace crates that depend on the client directly (normal deps only).
  parents=$(cargo tree --workspace -e normal -i "$client" --prefix none --depth 1 2>/dev/null \
    | grep -oE '^(os-[a-z]+|openbark-desktop|fixture-tts) v' | sed 's/ v$//' | sort -u || true)
  for p in $parents; do
    if [[ " $allowed " != *" $p "* ]]; then
      echo "::error::$p depends on HTTP client '$client'; only $allowed may (see docs/network.md)"
      violations=$((violations + 1))
    fi
  done
done
if [[ $violations -gt 0 ]]; then exit 1; fi
echo "network check: only [$allowed] has an HTTP client"
