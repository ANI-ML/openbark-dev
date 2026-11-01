#!/usr/bin/env bash
# Local-first guard (docs/network.md): only the crates named here may reach
# the network, and each only in its own way. Fails if any other workspace
# crate depends directly on an HTTP client, a TLS stack, an HTTP server, mDNS
# or raw sockets, or opens a socket in its source.
#
# HTTP clients:
#   os-models:  model downloads (R1, R2, R4, R5), and the version check
#               (R16; ADR-0035): one GET of www.openbark.io/release.json,
#               only from engine/os-models/src/update.rs, checked below
#   os-connect: the clinic's own Twilio account, REST to api.twilio.com only
#               (R9; ADR-0023)
#   os-sync:    encrypted sync: Supabase Auth and REST (R12, R13; being
#               retired by ADR-0029 phase D)
# The clinic hub (ADR-0029; docs/network.md L1 to L3):
#   os-hub:     the hub's TLS listener (L1) and mDNS advertising (L2);
#               desktop builds only
#   os-sync:    the device's pinned-TLS hub client (L3) and mDNS browsing (L2)
# Web mode:
#   os-server:  `openbark serve`, bound to 127.0.0.1 only
set -euo pipefail

violations=0

check_dep() {
  local dep=$1 allowed=$2 parents p
  # Workspace crates that depend on the crate directly (normal deps only).
  parents=$(cargo tree --workspace -e normal -i "$dep" --prefix none --depth 1 2>/dev/null \
    | grep -oE '^(os-[a-z]+|openbark-desktop|fixture-tts) v' | sed 's/ v$//' | sort -u || true)
  for p in $parents; do
    if [[ " $allowed " != *" $p "* ]]; then
      echo "::error::$p depends on '$dep'; only [$allowed] may (see docs/network.md)"
      violations=$((violations + 1))
    fi
  done
}

http_clients="os-models os-connect os-sync"
for client in reqwest ureq isahc attohttpc surf; do
  check_dep "$client" "$http_clients"
done
check_dep rustls "os-sync os-hub"
check_dep tokio-rustls "os-hub"
check_dep hyper "os-hub"
check_dep hyper-util "os-hub"
check_dep axum "os-server"
check_dep mdns-sd "os-sync os-hub"
check_dep socket2 "os-hub"
check_dep if-addrs "os-hub"

# Sockets in source: only in the crates above (tests included, which use
# loopback fakes).
socket_crates="os-models os-connect os-sync os-hub os-server"
mapfile -t files < <(grep -rlE '\b(TcpListener|TcpStream|UdpSocket)\b|socket2::' \
  engine/*/src apps/desktop/src-tauri/src tools/*/src 2>/dev/null || true)
for f in "${files[@]}"; do
  crate=$(echo "$f" | sed -nE 's#^engine/(os-[a-z]+)/.*#\1#p')
  if [[ -z "$crate" || " $socket_crates " != *" $crate "* ]]; then
    echo "::error::$f opens a socket; only [$socket_crates] may (see docs/network.md)"
    violations=$((violations + 1))
  fi
done

# The version check (R16, ADR-0035): its one address is named only in
# os-models/src/update.rs, so no other code (and not the WebView) reads it.
release_home="engine/os-models/src/update.rs"
mapfile -t release_files < <(grep -rlF 'openbark.io/release.json'   engine/*/src apps/desktop/src-tauri/src apps/desktop/src tools/*/src 2>/dev/null || true)
for f in "${release_files[@]}"; do
  if [[ "$f" != "$release_home" ]]; then
    echo "::error::$f names the version check's address; only $release_home may (see docs/network.md R16)"
    violations=$((violations + 1))
  fi
done

if [[ $violations -gt 0 ]]; then exit 1; fi
echo "network check: HTTP clients only in [$http_clients]; TLS, listeners and mDNS only in os-sync, os-hub and os-server (loopback); sockets only in [$socket_crates]; the version check's address only in $release_home"
