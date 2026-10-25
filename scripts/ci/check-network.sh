#!/usr/bin/env bash
# Local-first guard (docs/network.md): only the engine crates listed in
# `allowed` may have an HTTP client. Fails if any other workspace crate pulls
# in reqwest, ureq, hyper-based clients or similar.
#   os-models:  model downloads (R1, R2, R4, R5)
#   os-sync:    encrypted sync, Supabase Auth and REST only (R12, R13; ADR-0017)
#   os-connect: the clinic's own Twilio account, REST to api.twilio.com only
#               (R9; ADR-0023, docs/adr/0023-bring-your-own-telephony.md)
set -euo pipefail

allowed="os-models os-connect os-sync"
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
echo "network check: only [$allowed] have an HTTP client"
