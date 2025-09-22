#!/usr/bin/env bash
set -euo pipefail

REPORT_FILE="${ARW_LEGACY_CHECK_REPORT:-}"
declare -a REPORT_LINES=()

record() {
  REPORT_LINES+=("$1")
}

write_report() {
  [[ -n "${REPORT_FILE}" ]] || return 0
  mkdir -p "$(dirname "${REPORT_FILE}")"
  {
    echo "# Legacy Surface Check"
    echo "generated: $(date -Iseconds)"
    for entry in "${REPORT_LINES[@]}"; do
      printf '%s\n' "$entry"
    done
  } >"${REPORT_FILE}.tmp"
  mv "${REPORT_FILE}.tmp" "${REPORT_FILE}"
}

info() {
  echo "[legacy-check] $*"
  record "INFO: $*"
}

fail() {
  local message="$*"
  echo "[legacy-check] error: ${message}" >&2
  record "ERROR: ${message}"
  record "status: FAIL"
  record "completed: $(date -Iseconds)"
  write_report
  exit 1
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}" )/.." && pwd)"
PORT="${ARW_LEGACY_CHECK_PORT:-8119}"
WAIT_SECS="${ARW_LEGACY_CHECK_WAIT_SECS:-300}"
STATE_DIR="$(mktemp -d)"
LOG_FILE="$(mktemp)"
BUILD_LOG="$(mktemp)"

record "started: $(date -Iseconds)"
record "port: ${PORT}"
record "wait_timeout_secs: ${WAIT_SECS}"

cleanup() {
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  rm -rf "${STATE_DIR}" "${LOG_FILE}" "${BUILD_LOG}" 2>/dev/null || true
}
trap cleanup EXIT

export ARW_PORT="${PORT}"
export ARW_STATE_DIR="${STATE_DIR}"

info "building arw-server (debug profile)"
if ! (
  cd "${ROOT}" && cargo build -p arw-server >"${BUILD_LOG}" 2>&1
); then
  sed 's/^/[build] /' "${BUILD_LOG}" >&2 || true
  fail "failed to build arw-server"
fi

SERVER_BIN="${ROOT}/target/debug/arw-server"
[[ -x "${SERVER_BIN}" ]] || fail "compiled server binary not found at ${SERVER_BIN}"

info "starting arw-server on port ${PORT}"
"${SERVER_BIN}" --port "${PORT}" >"${LOG_FILE}" 2>&1 &
SERVER_PID=$!

record "server_pid: ${SERVER_PID}"

BASE="http://127.0.0.1:${PORT}"
deadline=$(( $(date +%s) + WAIT_SECS ))
while [[ $(date +%s) -lt ${deadline} ]]; do
  if curl -fsS "${BASE}/healthz" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "${SERVER_PID}" >/dev/null 2>&1; then
    sed 's/^/[server] /' "${LOG_FILE}" >&2 || true
    fail "server exited before becoming healthy"
  fi
  sleep 1
done

if ! curl -fsS "${BASE}/healthz" >/dev/null 2>&1; then
  sed 's/^/[server] /' "${LOG_FILE}" >&2 || true
  fail "server did not become healthy within ${WAIT_SECS}s"
fi

info "/debug should be unavailable"
code=$(curl -s -o /dev/null -w '%{http_code}' "${BASE}/debug")
[[ "${code}" == "404" ]] || fail "expected GET /debug → 404, got ${code}"
record "check: GET /debug → ${code}"

info "/admin/debug remains reachable"
code=$(curl -s -o /dev/null -w '%{http_code}' "${BASE}/admin/debug")
[[ "${code}" == "200" ]] || fail "expected GET /admin/debug → 200, got ${code}"
record "check: GET /admin/debug → ${code}"

info "legacy capsule header must be rejected"
code=$(curl -s -o /dev/null -w '%{http_code}' -H 'X-ARW-Gate: {}' "${BASE}/about")
[[ "${code}" == "410" ]] || fail "expected legacy capsule header → 410, got ${code}"
record "check: legacy capsule header → ${code}"

sleep 1
metric=$(curl -fsS "${BASE}/metrics" | awk '/^arw_legacy_capsule_headers_total /{print $2; exit}')
[[ -n "${metric}" ]] || fail "missing arw_legacy_capsule_headers_total counter"

info "legacy capsule counter reported ${metric}"
record "metric: arw_legacy_capsule_headers_total=${metric}"

if (( $(printf '%.0f' "${metric}" ) < 1 )); then
  fail "legacy capsule counter did not increment"
fi

info "legacy surface checks passed"
record "status: PASS"
record "completed: $(date -Iseconds)"
write_report
