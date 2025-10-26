#!/usr/bin/env bash
set -euo pipefail

# Sign an adapter manifest with an RSA private key via openssl.
# Usage: bash scripts/adapters_sign.sh <manifest> <private_key.pem>

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <manifest.(json|toml)> <private_key.pem>" >&2
  exit 2
fi

MANIFEST="$1"
KEY="$2"
if ! command -v openssl >/dev/null 2>&1; then
  echo "openssl not found in PATH" >&2
  exit 127
fi
if [[ ! -f "$MANIFEST" ]]; then echo "missing manifest: $MANIFEST" >&2; exit 2; fi
if [[ ! -f "$KEY" ]]; then echo "missing private key: $KEY" >&2; exit 2; fi

sig_bin="$MANIFEST.sig"
sig_b64="$MANIFEST.sig.b64"

openssl dgst -sha256 -sign "$KEY" -out "$sig_bin" "$MANIFEST"
openssl base64 -A -in "$sig_bin" -out "$sig_b64"
echo "wrote: $sig_bin and $sig_b64"

