#!/usr/bin/env bash
# A local Supabase for the encrypted sync tests (ADR-0017, M3 phase 2).
#
# Copies this repository's supabase/ folder (config, migrations, functions,
# email template) into a scratch project and runs the Supabase CLI there:
#
#   bash scripts/sync-local-supabase.sh start   # start (or restart) and reset the database
#   bash scripts/sync-local-supabase.sh reset   # re-apply the migrations to an empty database
#   bash scripts/sync-local-supabase.sh env     # print the test variables (eval it)
#   bash scripts/sync-local-supabase.sh stop
#
# `env` prints, for the Rust sync tests and the end-to-end script:
#   OPENBARK_SYNC_TEST_URL     http://127.0.0.1:54321
#   OPENBARK_SYNC_TEST_KEY     the local publishable (anon) key
#   OPENBARK_SYNC_TEST_SECRET  the local secret (service role) key: tests use it
#                              only to make sign-in codes and to read raw rows
#   OPENBARK_SYNC_TEST_MAIL    http://127.0.0.1:54324 (Mailpit)
# The keys are the CLI's local development defaults, read at run time; they
# are never written into the repository.
#
# Where it runs: Linux or macOS with Docker, directly; on Windows, inside WSL
# (OPENBARK_WSL_DISTRO, default Ubuntu-24.04), where Docker Engine runs. The
# CLI must be installed (pinned in docs/versions.md); on Windows the ports
# reach the host through WSL's localhost forwarding. Keep a WSL process
# running meanwhile (for example `wsl -d Ubuntu-24.04 -- sleep 86400`), or
# WSL may stop the VM and the containers with it.
#
# Never point this at a hosted project: it resets the database.
set -euo pipefail

cmd="${1:-start}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch='$HOME/openbark-sb'
cli='$HOME/.local/bin/supabase'

if [[ "$(uname -s)" == MINGW* || "$(uname -s)" == MSYS* ]]; then
  distro="${OPENBARK_WSL_DISTRO:-Ubuntu-24.04}"
  src="$(MSYS_NO_PATHCONV=1 wsl -d "$distro" -- wslpath -a "$(cygpath -m "$here/supabase")" | tr -d '\r')"
  run() { MSYS_NO_PATHCONV=1 wsl -d "$distro" -- bash -lc "$1"; }
else
  src="$here/supabase"
  run() { bash -lc "$1"; }
fi

copy="rm -rf $scratch/supabase && mkdir -p $scratch && cp -r '$src' $scratch/supabase"

case "$cmd" in
  start)
    run "$copy && cd $scratch && ($cli stop --no-backup >/dev/null 2>&1 || true) && $cli start && $cli db reset"
    ;;
  reset)
    run "$copy && cd $scratch && $cli db reset"
    ;;
  stop)
    run "cd $scratch && $cli stop --no-backup"
    ;;
  env)
    run "cd $scratch && $cli status -o env 2>/dev/null" | tr -d '\r' | awk -F= '
      $1 == "API_URL" { gsub(/"/, "", $2); print "export OPENBARK_SYNC_TEST_URL=" $2 }
      $1 == "PUBLISHABLE_KEY" || ($1 == "ANON_KEY" && !pk) { gsub(/"/, "", $2); key = $2; if ($1 == "PUBLISHABLE_KEY") pk = 1 }
      $1 == "SECRET_KEY" || ($1 == "SERVICE_ROLE_KEY" && !sk) { gsub(/"/, "", $2); secret = $2; if ($1 == "SECRET_KEY") sk = 1 }
      $1 == "MAILPIT_URL" || $1 == "INBUCKET_URL" { gsub(/"/, "", $2); mail = $2 }
      END {
        print "export OPENBARK_SYNC_TEST_KEY=" key
        print "export OPENBARK_SYNC_TEST_SECRET=" secret
        print "export OPENBARK_SYNC_TEST_MAIL=" (mail ? mail : "http://127.0.0.1:54324")
      }'
    ;;
  *)
    echo "usage: $0 start|reset|env|stop" >&2
    exit 2
    ;;
esac
