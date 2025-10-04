#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$SCRIPT_DIR/lib/smoke_timeout.sh"
smoke_timeout::init "sse-smoke" 600 "SSE_SMOKE_TIMEOUT_SECS"

cleanup() {
  local status=$?
  status=$(smoke_timeout::cleanup "$status")
  return "$status"
}
trap cleanup EXIT

run_command() {
  "$@" &
  local child=$!
  smoke_timeout::register_child "$child"
  set +e
  wait "$child"
  local status=$?
  set -e
  smoke_timeout::unregister_child "$child"
  return "$status"
}

# Simple SSE smoke test for unified server.
# Usage: ARW_ADMIN_TOKEN=... ./scripts/sse_smoke.sh [BASE]

BASE="${1:-http://127.0.0.1:8091}"

hdr_auth=()
if [[ -n "${ARW_ADMIN_TOKEN:-}" ]]; then
  hdr_auth=( -H "Authorization: Bearer ${ARW_ADMIN_TOKEN}" )
fi

echo "Connecting to $BASE/events (prefix=egress., replay=5)" >&2
run_command curl -N -sS "${BASE}/events?prefix=egress.&replay=5" "${hdr_auth[@]}"
