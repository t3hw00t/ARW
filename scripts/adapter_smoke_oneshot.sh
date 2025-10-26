#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${ARW_MOCK_ADAPTER_PORT:-8081}"
OUT="${ADAPTER_SMOKE_OUT:-}"

log() { printf '[adapters-oneshot] %s\n' "$*"; }

cd "$PROJECT_ROOT"

log "Building mock adapter health server..."
cargo build -p arw-mock-adapter >/dev/null

log "Starting mock adapter health on port ${PORT}..."
set +e
nohup "${PROJECT_ROOT}/target/debug/mock-adapter-health" >/tmp/mock-adapter.log 2>&1 &
PID=$!
set -e
trap 'kill "$PID" >/dev/null 2>&1 || true' EXIT INT TERM

for i in $(seq 1 30); do
  if curl -fsS -m 2 "http://127.0.0.1:${PORT}/healthz" >/dev/null; then
    log "mock adapter up"
    break
  fi
  sleep 1
done

log "Running adapter smoke (with health)..."
ADAPTER_SMOKE_HEALTH=1 ADAPTER_SMOKE_OUT="$OUT" bash "${PROJECT_ROOT}/scripts/adapter_smoke.sh"

log "Done. Stopping mock server (pid=${PID})."
kill "$PID" >/dev/null 2>&1 || true
trap - EXIT INT TERM
exit 0

