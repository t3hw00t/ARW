#!/usr/bin/env bash
set -euo pipefail

# Demonstrate generating an ed25519 keypair, creating a capsule, signing it,
# adding the pubkey to trust_capsules.json, and sending it to the service.

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT_DIR"

PORT=${ARW_PORT:-8090}
ISSUER=${ISSUER:-local-admin}
CAP_JSON=${CAP_JSON:-/tmp/arw-capsule.json}

echo "Generating ed25519 keypair..."
readarray -t LINES < <(cargo run -q -p arw-cli -- capsule gen-ed25519)
PUB=$(echo "${LINES[2]}" | sed 's/^pubkey_b64=//')
PRV=$(echo "${LINES[3]}" | sed 's/^privkey_b64=//')
echo "PUB=$PUB"

echo "Writing trust_capsules.json..."
cat > configs/trust_capsules.json <<JSON
{
  "issuers": [
    { "id": "$ISSUER", "alg": "ed25519", "key_b64": "$PUB" }
  ]
}
JSON

echo "Creating capsule template..."
cargo run -q -p arw-cli -- capsule template > "$CAP_JSON"
echo "Signing capsule..."
SIG=$(cargo run -q -p arw-cli -- capsule sign-ed25519 "$PRV" "$CAP_JSON")
echo "Signature: $SIG"

echo "Sending capsule in header to /healthz (admin-gated routes recommended)..."
HDR=$(jq -c --arg sig "$SIG" '.signature=$sig' "$CAP_JSON")
curl -s -H "X-ARW-Gate: $HDR" http://127.0.0.1:$PORT/healthz || true
echo
echo "Done. Capsule adopted ephemerally until restart."

