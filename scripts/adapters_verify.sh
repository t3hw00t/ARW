#!/usr/bin/env bash
set -euo pipefail

# Verify an adapter manifest signature with an RSA public key via openssl.
# Usage: bash scripts/adapters_verify.sh <manifest> <public_key.pem> [sig.b64|sig]

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <manifest.(json|toml)> <public_key.pem> [signature file]" >&2
  exit 2
fi

MANIFEST="$1"
PUBKEY="$2"
SIGFILE="${3:-$MANIFEST.sig}"

if ! command -v openssl >/dev/null 2>&1; then
  echo "openssl not found in PATH" >&2
  exit 127
fi
if [[ ! -f "$MANIFEST" ]]; then echo "missing manifest: $MANIFEST" >&2; exit 2; fi
if [[ ! -f "$PUBKEY" ]]; then echo "missing public key: $PUBKEY" >&2; exit 2; fi
if [[ ! -f "$SIGFILE" ]]; then echo "missing signature: $SIGFILE" >&2; exit 2; fi

tmp_sig=""
case "$SIGFILE" in
  *.b64) tmp_sig="$(mktemp)"; openssl base64 -d -in "$SIGFILE" -out "$tmp_sig" ;;
  *) tmp_sig="$SIGFILE" ;;
esac

set +e
openssl dgst -sha256 -verify "$PUBKEY" -signature "$tmp_sig" "$MANIFEST"
code=$?
set -e
if [[ -n "$tmp_sig" && "$tmp_sig" != "$SIGFILE" ]]; then rm -f "$tmp_sig"; fi
exit $code

