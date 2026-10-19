#!/usr/bin/env bash
# OpenPGP signatures for a release (ADR-0020, decision 4).
#
#   scripts/ci/gpg-sign.sh checksums <SHA256SUMS.txt> <public-key-out.asc>
#       Writes <SHA256SUMS.txt>.asc (detached, armoured) and exports the public
#       key, then checks the signature against that exported key alone.
#   scripts/ci/gpg-sign.sh rpm <file.rpm>...
#       Signs each package in place with rpmsign, then checks it with `rpm -K`
#       against a throwaway rpm database that holds only the public key.
#
# Environment:
#   OPENBARK_RELEASE_GPG_KEY          armoured secret key (required)
#   OPENBARK_RELEASE_GPG_PASSPHRASE   its passphrase (empty if it has none)
#   OPENBARK_RELEASE_GPG_FINGERPRINT  optional; if set, the key must match it
#
# The key lives in a temporary GNUPGHOME that is deleted on exit. The
# passphrase goes to gpg-agent through a pipe (gpg-preset-passphrase), never
# into a file, an argument or the log.
set -euo pipefail
umask 077

MODE="${1:-}"
shift || true
[ -n "${OPENBARK_RELEASE_GPG_KEY:-}" ] || { echo "OPENBARK_RELEASE_GPG_KEY is empty."; exit 1; }

TMP="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
export GNUPGHOME
GNUPGHOME="$(mktemp -d "$TMP/obgpg.XXXXXX")"
VERIFY_HOME="$(mktemp -d "$TMP/obgpgv.XXXXXX")"
RPMDB="$(mktemp -d "$TMP/obrpmdb.XXXXXX")"
cleanup() {
  gpgconf --homedir "$GNUPGHOME" --kill all 2>/dev/null || true
  gpgconf --homedir "$VERIFY_HOME" --kill all 2>/dev/null || true
  rm -rf "$GNUPGHOME" "$VERIFY_HOME" "$RPMDB"
}
trap cleanup EXIT

printf 'allow-preset-passphrase\nallow-loopback-pinentry\n' > "$GNUPGHOME/gpg-agent.conf"
printf '%s\n' "$OPENBARK_RELEASE_GPG_KEY" | gpg --batch --quiet --import 2>/dev/null \
  || { echo "OPENBARK_RELEASE_GPG_KEY couldn't be imported (is it the armoured secret key?)."; exit 1; }

FPR="$(gpg --batch --with-colons --list-secret-keys | awk -F: '$1 == "fpr" { print $10; exit }')"
[ -n "$FPR" ] || { echo "OPENBARK_RELEASE_GPG_KEY holds no secret key."; exit 1; }
if [ -n "${OPENBARK_RELEASE_GPG_FINGERPRINT:-}" ]; then
  want="$(printf '%s' "$OPENBARK_RELEASE_GPG_FINGERPRINT" | tr -d ' \t' | tr '[:lower:]' '[:upper:]')"
  [ "$want" = "$FPR" ] || { echo "The key's fingerprint $FPR doesn't match OPENBARK_RELEASE_GPG_FINGERPRINT ($want)."; exit 1; }
fi
echo "Signing key: $FPR"

if [ -n "${OPENBARK_RELEASE_GPG_PASSPHRASE:-}" ]; then
  preset="$(gpgconf --list-dirs libexecdir)/gpg-preset-passphrase"
  gpg-connect-agent /bye >/dev/null 2>&1
  for grip in $(gpg --batch --with-colons --with-keygrip --list-secret-keys | awk -F: '$1 == "grp" { print $10 }'); do
    printf '%s' "$OPENBARK_RELEASE_GPG_PASSPHRASE" | "$preset" --preset "$grip"
  done
fi

export_public() {
  gpg --batch --armor --export "$FPR" > "$1"
  [ -s "$1" ] || { echo "Couldn't export the public key."; exit 1; }
  chmod 644 "$1"
}

case "$MODE" in
  checksums)
    sums="${1:?checksums file}"
    pub="${2:?public key output path}"
    [ -f "$sums" ] || { echo "No such file: $sums"; exit 1; }
    gpg --batch --yes --pinentry-mode loopback --local-user "$FPR" --digest-algo SHA256 \
      --armor --detach-sign --output "$sums.asc" "$sums"
    chmod 644 "$sums.asc"
    export_public "$pub"
    # Check with nothing but the exported public key, as a user would.
    gpg --homedir "$VERIFY_HOME" --batch --quiet --import "$pub"
    status="$(gpg --homedir "$VERIFY_HOME" --batch --status-fd 1 --verify "$sums.asc" "$sums" 2>/dev/null)" \
      || { echo "gpg --verify failed on $sums.asc"; exit 1; }
    printf '%s\n' "$status" | grep -q "^\[GNUPG:\] VALIDSIG $FPR " \
      || { echo "The signature on $sums isn't from $FPR."; exit 1; }
    echo "Signed $(basename "$sums") -> $(basename "$sums").asc; gpg --verify: good signature from $FPR"
    ;;
  rpm)
    [ $# -gt 0 ] || { echo "No .rpm files given."; exit 1; }
    command -v rpmsign >/dev/null || { echo "rpmsign is missing (apt-get install rpm)."; exit 1; }
    pub="$GNUPGHOME/release-key.asc"
    export_public "$pub"
    rpm --dbpath "$RPMDB" --initdb
    rpm --dbpath "$RPMDB" --import "$pub"
    for f in "$@"; do
      # Debian and Ubuntu's rpm points __gpg at /usr/bin/gpg2, which they don't ship.
      rpmsign --addsign \
        --define "__gpg $(command -v gpg)" \
        --define "_gpg_path $GNUPGHOME" \
        --define "_gpg_name $FPR" \
        --define "_gpg_digest_algo sha256" \
        --define "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback" \
        "$f" >/dev/null
      out="$(rpm --dbpath "$RPMDB" -Kv "$f")"
      printf '%s\n' "$out"
      if printf '%s\n' "$out" | grep -q 'NOT OK' || ! printf '%s\n' "$out" | grep -qi 'signature, key id.*: OK'; then
        echo "rpm -K doesn't accept the signature on $f."
        exit 1
      fi
      echo "Signed $(basename "$f"); rpm -K: signature OK"
    done
    ;;
  *)
    sed -n '2,20p' "$0"
    exit 2
    ;;
esac
