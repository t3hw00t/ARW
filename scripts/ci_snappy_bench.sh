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

export QUEUE_BUDGET_MS FULL_BUDGET_MS

STATE_DIR="$(mktemp -d)"
if [[ -n "${RUNNER_TEMP:-}" ]]; then
  BENCH_JSON="${RUNNER_TEMP}/snappy-bench-summary.json"
else
  BENCH_JSON="$(mktemp "${TMPDIR:-/tmp}/snappy-bench-XXXXXX.json")"
fi
export BENCH_JSON

server_pid=""
cleanup() {
  if [[ -n "${server_pid}" ]]; then
    kill "${server_pid}" >/dev/null 2>&1 || true
    wait "${server_pid}" 2>/dev/null || true
  fi
  if [[ -d "${STATE_DIR}" ]]; then
    rm -rf "${STATE_DIR}"
  fi
  if [[ -z "${RUNNER_TEMP:-}" && -f "${BENCH_JSON}" ]]; then
    rm -f "${BENCH_JSON}"
  fi
}
trap cleanup EXIT

export ARW_ADMIN_TOKEN="${TOKEN}"
export ARW_PORT="${PORT}"
export ARW_DEBUG="0"
export ARW_STATE_DIR="${STATE_DIR}"

# Ensure release binaries are available (keeps CI latency predictable).
SERVER_BIN="${ROOT}/target/release/arw-server"
BENCH_BIN="${ROOT}/target/release/snappy-bench"
if [[ ! -x "${SERVER_BIN}" || ! -x "${BENCH_BIN}" ]]; then
  echo "[snappy-bench] building release binaries"
  (cd "${ROOT}" && cargo build --release -p arw-server -p snappy-bench >/tmp/snappy-bench-build.log 2>&1) || {
    echo "[snappy-bench] build failed" >&2
    sed 's/^/[build] /' /tmp/snappy-bench-build.log >&2 || true
    exit 1
  }
fi

# Start the unified server in the background.
"${SERVER_BIN}" --port "${PORT}" >/tmp/snappy-bench-server.log 2>&1 &
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

"${BENCH_BIN}" \
  --base "http://127.0.0.1:${PORT}" \
  --admin-token "${TOKEN}" \
  --requests "${REQUESTS}" \
  --concurrency "${CONCURRENCY}" \
  --json-out "${BENCH_JSON}" \
  --budget-queue-ms "${QUEUE_BUDGET_MS}" \
  --budget-full-ms "${FULL_BUDGET_MS}"

echo "[snappy-bench] bench run completed"

echo "[snappy-bench] parsed summary:"
python3 <<'PY'
import json, os
path = os.environ["BENCH_JSON"]
with open(path, "r", encoding="utf-8") as fh:
    data = json.load(fh)
summary = {
    "requests": data.get("requests"),
    "completed": data.get("completed"),
    "failed": data.get("failed"),
    "queue_p95_ms": (data.get("latency_ms", {}) or {}).get("queue", {}).get("p95"),
    "total_p95_ms": (data.get("latency_ms", {}) or {}).get("total", {}).get("p95"),
    "throughput_per_sec": data.get("throughput_per_sec"),
}
print(json.dumps(summary, indent=2))

# fail fast if totals exceed budgets; the bench already returned non-zero
# earlier, but we enforce again here using the parsed JSON for clarity
if summary["total_p95_ms"] is not None and summary["total_p95_ms"] > float(os.environ["FULL_BUDGET_MS"]):
    raise SystemExit("total p95 exceeded budget")
if summary["queue_p95_ms"] is not None and summary["queue_p95_ms"] > float(os.environ["QUEUE_BUDGET_MS"]):
    raise SystemExit("queue p95 exceeded budget")
PY

echo "[snappy-bench] JSON summary stored in ${BENCH_JSON}"
