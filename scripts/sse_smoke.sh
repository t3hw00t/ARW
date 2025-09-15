#!/usr/bin/env bash
set -euo pipefail

# Simple SSE smoke test for unified server.
# Usage: ARW_ADMIN_TOKEN=... ./scripts/sse_smoke.sh [BASE]

BASE="${1:-http://127.0.0.1:8091}"

hdr_auth=()
if [[ -n "${ARW_ADMIN_TOKEN:-}" ]]; then
  hdr_auth=( -H "Authorization: Bearer ${ARW_ADMIN_TOKEN}" )
fi

echo "Connecting to $BASE/events (prefix=egress., replay=5)" >&2
exec curl -N -sS "${BASE}/events?prefix=egress.&replay=5" "${hdr_auth[@]}"

