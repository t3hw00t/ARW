#!/usr/bin/env bash
set -euo pipefail

# Demo: start NATS, service (with NATS), and one connector.
# Requires: nats-server in PATH.

ROOT_DIR=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT_DIR"

NATS_URL=${NATS_URL:-nats://127.0.0.1:4222}
PORT=${ARW_PORT:-8090}
NODE_ID=${ARW_NODE_ID:-core-a}

logdir=${LOG_DIR:-/tmp/arw-demo}
mkdir -p "$logdir"

cleanup() {
  echo "Stopping demo..."
  pkill -f "nats-server" || true
  pkill -f "arw-svc" || true
  pkill -f "arw-connector" || true
}
trap cleanup EXIT

echo "Starting NATS..."
if ! command -v nats-server >/dev/null 2>&1; then
  echo "nats-server not found in PATH; please install NATS to run cluster demo" >&2
  exit 1
fi
nohup nats-server -p 4222 >"$logdir/nats.log" 2>&1 &
sleep 1

echo "Starting arw-svc..."
ARW_DEBUG=1 ARW_PORT=$PORT ARW_NODE_ID=$NODE_ID ARW_NATS_OUT=1 \
  cargo run -q -p arw-svc --features nats >"$logdir/svc.log" 2>&1 &
sleep 2

echo "Starting arw-connector..."
ARW_NODE_ID=worker-1 ARW_NATS_URL=$NATS_URL \
  cargo run -q -p arw-connector --features nats >"$logdir/connector.log" 2>&1 &

echo "Demo running. Service: http://127.0.0.1:$PORT"
echo "Logs in $logdir"
echo "Try: curl -s -X POST -H 'content-type: application/json' http://127.0.0.1:$PORT/tasks/enqueue -d '{"kind":"math.add","payload":{"a":1,"b":2}}'"
echo "Press Ctrl-C to stop."
wait

