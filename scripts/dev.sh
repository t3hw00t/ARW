#!/usr/bin/env bash
set -euo pipefail

# Usage: scripts/dev.sh [PORT] [DOCS_ADDR]
# Defaults: PORT=8090, DOCS_ADDR=127.0.0.1:8000

PORT=${1:-${ARW_PORT:-8090}}
DOCS_ADDR=${2:-${DOCS_ADDR:-127.0.0.1:8000}}
DOCS_URL="http://${DOCS_ADDR}"

echo "[dev] Using PORT=${PORT} · DOCS=${DOCS_URL}"

pids=()
cleanup() {
  trap - INT TERM EXIT
  for pid in "${pids[@]:-}"; do
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" || true
    fi
  done
}
trap cleanup INT TERM EXIT

echo "[dev] Starting docs: mkdocs serve -a ${DOCS_ADDR}"
mkdocs serve -a "${DOCS_ADDR}" &
pids+=($!)

echo "[dev] Starting service: ARW_DEBUG=1 ARW_DOCS_URL=${DOCS_URL} ARW_PORT=${PORT} cargo run -p arw-svc"
ARW_DEBUG=1 ARW_DOCS_URL="${DOCS_URL}" ARW_PORT="${PORT}" cargo run -p arw-svc &
pids+=($!)

wait -n "${pids[@]}"
echo "[dev] One process exited; shutting down..."
cleanup
wait || true

