#!/usr/bin/env bash
set -euo pipefail

# Run a lightweight snappy bench against a locally started arw-server.
# Intended for CI/nightly sanity checks.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${ARW_BENCH_PORT:-8091}"
TOKEN="${ARW_BENCH_TOKEN:-ci-bench}"
REQUESTS="${ARW_BENCH_REQUESTS:-60}"
CONCURRENCY="${ARW_BENCH_CONCURRENCY:-6}"
WAIT_HEALTH_TIMEOUT="${ARW_BENCH_HEALTH_TIMEOUT:-30}"
QUEUE_BUDGET_MS="${ARW_BENCH_QUEUE_BUDGET_MS:-500}"
FULL_BUDGET_MS="${ARW_BENCH_FULL_BUDGET_MS:-2000}"

STATE_DIR="$(mktemp -d)"

server_pid=""
cleanup() {
  if [[ -n "${server_pid}" ]]; then
    kill "${server_pid}" >/dev/null 2>&1 || true
    wait "${server_pid}" 2>/dev/null || true
  fi
  if [[ -d "${STATE_DIR}" ]]; then
    rm -rf "${STATE_DIR}"
  fi
}
trap cleanup EXIT

export ARW_ADMIN_TOKEN="${TOKEN}"
export ARW_PORT="${PORT}"
export ARW_DEBUG="0"
export ARW_STATE_DIR="${STATE_DIR}"

# Start the unified server in the background (dev profile is sufficient here).
(
  cd "${ROOT}"
  cargo run -p arw-server -- --port "${PORT}" >/tmp/snappy-bench-server.log 2>&1
) &
server_pid=$!

echo "[snappy-bench] arw-server spawned (pid=${server_pid}), waiting for healthz..."

health_deadline=$(( $(date +%s) + WAIT_HEALTH_TIMEOUT ))
while true; do
  if curl -fsS "http://127.0.0.1:${PORT}/healthz" >/dev/null 2>&1; then
    break
  fi
  if [[ $(date +%s) -ge ${health_deadline} ]]; then
    echo "[snappy-bench] server failed to become healthy within ${WAIT_HEALTH_TIMEOUT}s" >&2
    echo "[snappy-bench] server logs:" >&2
    sed 's/^/[server] /' /tmp/snappy-bench-server.log >&2 || true
    exit 1
  fi
  sleep 1
  if ! kill -0 "${server_pid}" >/dev/null 2>&1; then
    echo "[snappy-bench] server exited early" >&2
    sed 's/^/[server] /' /tmp/snappy-bench-server.log >&2 || true
    exit 1
  fi
  done

echo "[snappy-bench] server healthy, running bench (requests=${REQUESTS}, concurrency=${CONCURRENCY})"

(
  cd "${ROOT}"
  cargo run -p snappy-bench -- \
    --base "http://127.0.0.1:${PORT}" \
    --admin-token "${TOKEN}" \
    --requests "${REQUESTS}" \
    --concurrency "${CONCURRENCY}" \
    --budget-queue-ms "${QUEUE_BUDGET_MS}" \
    --budget-full-ms "${FULL_BUDGET_MS}"
)

echo "[snappy-bench] bench run completed"
