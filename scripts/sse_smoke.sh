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
REPLAY="${SSE_SMOKE_REPLAY:-5}"
EXPECT_EVENT="${SSE_SMOKE_EXPECT_EVENT:-service.connected}"
STREAM_TIMEOUT="${SSE_SMOKE_STREAM_TIMEOUT:-12}"

probe_sse() {
  local base="$1"
  local token="$2"
  local replay="$3"
  local expect_event="$4"
  local timeout="$5"

  python3 - "$base" "$token" "$replay" "$expect_event" "$timeout" <<'PY'
import http.client
import sys
import time
import urllib.parse

base, token, replay, expect_event, timeout = sys.argv[1:]
timeout = float(timeout or 12)
replay = replay or "5"
parsed = urllib.parse.urlparse(base)
if not parsed.scheme.startswith("http"):
    raise SystemExit(f"unsupported scheme in base URL: {base!r}")

path = parsed.path.rstrip("/") + "/events"
query = f"?prefix=egress.&replay={replay}"
target = f"{path}{query}"

headers = {
    "Accept": "text/event-stream",
    "Cache-Control": "no-cache",
}
if token:
    headers["Authorization"] = f"Bearer {token}"

conn_cls = http.client.HTTPSConnection if parsed.scheme == "https" else http.client.HTTPConnection
host = parsed.hostname
port = parsed.port
if port is None:
    port = 443 if parsed.scheme == "https" else 80

conn = conn_cls(host, port, timeout=timeout)
try:
    conn.request("GET", target, headers=headers)
    resp = conn.getresponse()
    if resp.status != 200:
        body = resp.read(160).decode("utf-8", "ignore")
        raise SystemExit(f"SSE request failed ({resp.status}): {body!r}")

    expect_token = f"event: {expect_event}"
    deadline = time.time() + timeout
    buffer = ""
    while time.time() < deadline:
        line = resp.readline().decode("utf-8", "ignore")
        if not line:
            break
        buffer += line
        if expect_token in buffer:
            print(f"[sse-smoke] observed {expect_token!r}")
            break
        if len(buffer) > 32_768:
            buffer = buffer[-32_768:]
    else:
        raise SystemExit(f"SSE stream did not emit {expect_event!r} within {timeout}s")

finally:
    conn.close()
PY
}

echo "Connecting to $BASE/events (prefix=egress., replay=${REPLAY})" >&2
run_command probe_sse "$BASE" "${ARW_ADMIN_TOKEN:-}" "$REPLAY" "$EXPECT_EVENT" "$STREAM_TIMEOUT"
