#!/usr/bin/env bash
# Decide whether this release job signs (ADR-0020, decisions 7 and 8).
#
#   scripts/ci/signing-plan.sh <windows|macos|linux|checksums>
#
# The workflow passes only true/false flags, never secret values:
#   IS_RELEASE_TAG    true for a pushed v* tag (manual runs never sign)
#   HAS_<NAME>        true if the secret or variable <NAME> is set
# Signing turns on by itself once a platform's secrets exist in the GitHub
# `release` environment. With none of them, the platform builds unsigned as
# before, with a notice in the log. With only some of them, the job stops:
# a half-configured platform is a mistake to fix, not a reason to ship
# unsigned without anyone noticing.
#
# Writes sign=true|false to GITHUB_OUTPUT, and signing-status/<platform>.txt
# for the release notes.
set -euo pipefail

platform="${1:?platform}"
case "$platform" in
  windows)   required="AZURE_CLIENT_ID AZURE_TENANT_ID AZURE_SUBSCRIPTION_ID AZURE_SIGNING_ENDPOINT AZURE_SIGNING_ACCOUNT AZURE_SIGNING_PROFILE"
             what="Windows (Microsoft Artifact Signing)" ;;
  macos)     required="APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_KEY APPLE_API_ISSUER APPLE_API_PRIVATE_KEY"
             what="macOS (Developer ID and notarization)" ;;
  linux|checksums)
             required="OPENBARK_RELEASE_GPG_KEY"
             what="OpenPGP ($platform)" ;;
  *) echo "unknown platform: $platform"; exit 2 ;;
esac

present=""
missing=""
for name in $required; do
  var="HAS_$name"
  if [ "${!var:-false}" = "true" ]; then present="$present $name"; else missing="$missing $name"; fi
done

sign=false
if [ "${IS_RELEASE_TAG:-false}" != "true" ]; then
  echo "::notice title=Unsigned build ($platform)::Not a v* tag push, so nothing is signed. Signing runs only for release tags (ADR-0020)."
elif [ -z "$present" ]; then
  echo "::notice title=Unsigned build ($platform)::No $what secrets in the release environment, so this platform builds unsigned, as before. Adding them turns signing on (docs/runbooks/code-signing.md, Part D3)."
elif [ -n "$missing" ]; then
  echo "::error title=Signing half configured ($platform)::$what has some of its secrets but not:$missing. Add the missing ones, or remove the others to build unsigned."
  exit 1
else
  sign=true
  echo "Signing $what with:$present"
  if [ "$platform" = linux ] || [ "$platform" = checksums ]; then
    [ "${HAS_OPENBARK_RELEASE_GPG_PASSPHRASE:-false}" = "true" ] \
      || echo "::warning::OPENBARK_RELEASE_GPG_PASSPHRASE is empty; that only works for a key without a passphrase."
    [ "${HAS_OPENBARK_RELEASE_GPG_FINGERPRINT:-false}" = "true" ] \
      || echo "::warning::The OPENBARK_RELEASE_GPG_FINGERPRINT variable isn't set, so the key's fingerprint isn't checked."
  fi
fi

echo "sign=$sign" >> "${GITHUB_OUTPUT:-/dev/null}"
mkdir -p signing-status
if [ "$sign" = true ]; then echo signed > "signing-status/$platform.txt"; else echo unsigned > "signing-status/$platform.txt"; fi
