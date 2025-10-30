#!/usr/bin/env bash
set -euo pipefail

# Local deep checks harness (Linux/macOS)
# - Builds arw-server, arw-cli, arw-mini-dashboard (release)
# - Starts arw-server on ARW_PORT (default 8099)
# - Verifies economy snapshot JSON, route_stats (mini-dashboard once),
#   events tail structured output, and SSE metrics presence
# - Stops server and writes logs to /tmp

BASE="${BASE:-http://127.0.0.1:8099}"
ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-test-admin-token}"
export ARW_ADMIN_TOKEN
PORT="${ARW_PORT:-8099}"

echo "[deep-checks] building (release)"
cargo build -p arw-server -p arw-cli -p arw-mini-dashboard --release

echo "[deep-checks] starting arw-server on port ${PORT}"
# Ensure Linux mode and isolated state/cache/log dirs (WSL friendly)
mkdir -p /tmp/arw-data /tmp/arw-cache /tmp/arw-logs || true
ARW_ENV_MODE_FORCE=linux \
ARW_DATA_DIR=/tmp/arw-data \
ARW_CACHE_DIR=/tmp/arw-cache \
ARW_LOGS_DIR=/tmp/arw-logs \
ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" ARW_DEBUG=1 ARW_PORT="$PORT" \
nohup target/release/arw-server > /tmp/arw-svc-local.log 2>&1 &
echo $! > /tmp/arw-svc-local.pid

for i in $(seq 1 120); do
  if curl -fsS -m 2 "$BASE/healthz" >/dev/null; then echo "[deep-checks] server ready"; break; fi; sleep 1;
done
sleep 2
if ! curl -fsS "$BASE/about" >/tmp/about.json 2>/dev/null; then
  echo "[deep-checks] /about failed; server log tail:"; tail -n 120 /tmp/arw-svc-local.log || true; exit 1;
fi

echo "[deep-checks] economy snapshot"
target/release/arw-cli state economy-ledger --base "$BASE" --limit 5 --json > /tmp/economy.json
if command -v jq >/dev/null 2>&1; then
  jq -e '.version' /tmp/economy.json >/dev/null || { echo "economy.json missing version"; exit 1; }
else
  grep -q '"version"' /tmp/economy.json || { echo "economy.json missing version (no jq)"; exit 1; }
fi

echo "[deep-checks] route_stats snapshot"
if ! curl -fsS -H "Authorization: Bearer $ARW_ADMIN_TOKEN" "$BASE/state/route_stats" > /tmp/route_stats.json; then
  if [ "${DEEP_SOFT:-0}" = "1" ]; then echo "[deep-checks][soft] route_stats fetch failed; continuing"; else echo "route_stats fetch failed"; exit 1; fi
fi
if command -v jq >/dev/null 2>&1; then
  jq type /tmp/route_stats.json >/dev/null || { if [ "${DEEP_SOFT:-0}" = "1" ]; then echo "[deep-checks][soft] route_stats.json parse failed"; else echo "route_stats.json parse failed"; exit 1; fi }
else
  head -n 1 /tmp/route_stats.json >/dev/null || { if [ "${DEEP_SOFT:-0}" = "1" ]; then echo "[deep-checks][soft] route_stats.json empty"; else echo "route_stats.json empty"; exit 1; fi }
fi

echo "[deep-checks] events tail (SSE via curl)"
TAIL_SECS=${DEEP_TAIL_SECS:-10}
EVENTS_RAW=/tmp/events_raw.sse
EVENTS_JSON=/tmp/events_tail.json
rm -f "$EVENTS_RAW" "$EVENTS_JSON"

curl_args=(
  -fsS -N
  -H "Accept: text/event-stream"
  -H "Authorization: Bearer $ARW_ADMIN_TOKEN"
  "$BASE/events?prefix=service.,state."
)

if command -v timeout >/dev/null 2>&1; then
  timeout "${TAIL_SECS}s" curl "${curl_args[@]}" > "$EVENTS_RAW" || true
elif command -v gtimeout >/dev/null 2>&1; then
  gtimeout "${TAIL_SECS}s" curl "${curl_args[@]}" > "$EVENTS_RAW" || true
else
  curl "${curl_args[@]}" > "$EVENTS_RAW" &
  curl_pid=$!
  (
    sleep "$TAIL_SECS"
    kill "$curl_pid" >/dev/null 2>&1 || true
  ) &
  guard_pid=$!
  wait "$curl_pid" 2>/dev/null || true
  kill "$guard_pid" >/dev/null 2>&1 || true
fi

awk '/^data:/{sub(/^data:[ ]*/, ""); print}' "$EVENTS_RAW" > "$EVENTS_JSON" || true
if ! head -n 1 "$EVENTS_JSON" | grep -q '{'; then
  if [ "${DEEP_SOFT:-0}" = "1" ]; then
    echo "[deep-checks][soft] no events output; continuing due to DEEP_SOFT=1"
  else
    echo "no events output"; exit 1;
  fi
fi

echo "[deep-checks] metrics (SSE counters present)"
# Fetch metrics with a short retry to give exporters time to emit
rm -f /tmp/metrics.txt || true
for i in $(seq 1 5); do
  if curl -fsS "$BASE/metrics" > /tmp/metrics.txt 2>/dev/null; then break; fi; sleep 1;
done
if grep -E '^arw_events_sse_(clients|connections_total|sent_total)' /tmp/metrics.txt >/dev/null; then
  : # ok
else
  # Fallback: some local environments omit these lines; accept events_total patch counter as proxy
  if grep -E '^arw_events_total\{kind="state.read.model.patch"\}' /tmp/metrics.txt >/dev/null; then
    echo "[deep-checks] using events_total{kind=state.read.model.patch} as SSE proxy"
  else
    if [ "${DEEP_SOFT:-0}" = "1" ]; then
      echo "[deep-checks][soft] missing SSE metrics; continuing due to DEEP_SOFT=1"
    else
      echo "missing SSE metrics"; exit 1;
    fi
  fi
fi

echo "[deep-checks] stopping server"
if [ -f /tmp/arw-svc-local.pid ]; then kill "$(cat /tmp/arw-svc-local.pid)" || true; fi
echo "[deep-checks] logs at /tmp/arw-svc-local.log"

echo "[deep-checks] OK"
