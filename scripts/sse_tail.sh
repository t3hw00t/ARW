#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
source "$SCRIPT_DIR/lib/smoke_timeout.sh"
smoke_timeout::init "sse-tail" 600 "SSE_TAIL_TIMEOUT_SECS"

cleanup() {
  local status=$?
  status=$(smoke_timeout::cleanup "$status")
  return "$status"
}
trap cleanup EXIT

run_stream() {
  (
    curl -N -sS "${AUTH[@]}" "$URL" \
      | awk -v store="$STORE" '
          /^id:/ { sub(/^id: */,"",$0); print > store; next }
          /^data:/ { sub(/^data: */,"",$0); print $0; next }
        ' \
      | jq -rc '{id:.payload.id//empty, kind:.kind, payload:.payload}'
  ) &
  local child=$!
  smoke_timeout::register_child "$child"
  set +e
  wait "$child"
  local status=$?
  set -e
  smoke_timeout::unregister_child "$child"
  return "$status"
}

# Simple SSE tail helper using curl + jq
#
# Usage:
#   BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=... \
#   scripts/sse_tail.sh --prefix service.,state.read.model.patch --replay 25 --store .arw/last-event-id
#
# Flags:
#   --prefix CSV    Comma-separated event kind prefixes (e.g., service.,state.)
#   --replay N      Replay the last N events on connect (default 0)
#   --store FILE    Path to store Last-Event-ID for resume (default .arw/last-event-id)
#   --after ID      Resume after a specific event id (overrides --store)
#
# Requires: curl, jq

BASE=${BASE:-http://127.0.0.1:8091}
TOKEN=${ARW_ADMIN_TOKEN:-}
PREFIX=""
REPLAY=0
STORE=".arw/last-event-id"
AFTER=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --prefix) PREFIX="$2"; shift 2;;
    --replay) REPLAY="$2"; shift 2;;
    --store)  STORE="$2"; shift 2;;
    --after)  AFTER="$2"; shift 2;;
    -h|--help)
      sed -n '1,40p' "$0" | sed 's/^# //;t;d'; exit 0;;
    *) echo "unknown arg: $1" >&2; exit 2;;
  esac
done

if [[ -z "$AFTER" && -f "$STORE" ]]; then
  AFTER=$(sed -n '1p' "$STORE" 2>/dev/null || true)
fi

QS=""
if [[ -n "$PREFIX" ]]; then QS+="prefix=$PREFIX"; fi
if [[ "$REPLAY" != "0" ]]; then QS+="${QS:+&}replay=$REPLAY"; fi
if [[ -n "$AFTER" ]]; then QS+="${QS:+&}after=$AFTER"; fi
URL="$BASE/events${QS:+?$QS}"

AUTH=()
if [[ -n "$TOKEN" ]]; then AUTH=(-H "Authorization: Bearer $TOKEN"); fi

run_stream
