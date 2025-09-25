#!/usr/bin/env bash
set -euo pipefail

# Lightweight legacy-surface guard. We keep a static probe to ensure the unified
# router never reintroduces the legacy `/debug` alias and, when a server is
# already running, we optionally smoke a few HTTP paths. No builds or child
# servers are spawned here—run against an existing instance if you want runtime
# checks.

command -v rg >/dev/null 2>&1 || {
  echo "[legacy-check] error: ripgrep (rg) is required" >&2
  exit 1
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}" )/.." && pwd)"
REPORT_FILE="${ARW_LEGACY_CHECK_REPORT:-}"
BASE="${ARW_LEGACY_CHECK_BASE:-http://127.0.0.1:8091}"

reset_report() {
  [[ -n "${REPORT_FILE}" ]] || return 0
  mkdir -p "$(dirname "${REPORT_FILE}")"
  {
    echo "# Legacy Surface Check"
    echo "generated: $(date -Iseconds)"
  } >"${REPORT_FILE}"
}

record() {
  [[ -n "${REPORT_FILE}" ]] || return 0
  printf '%s\n' "$1" >>"${REPORT_FILE}"
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
  exit 1
}

reset_report
record "started: $(date -Iseconds)"

# --- Static probe ----------------------------------------------------------

if rg --no-heading --line-number '"/debug"' "${ROOT}/apps/arw-server/src/router.rs" >/dev/null 2>&1; then
  fail "router exposes legacy /debug alias"
fi

info "router alias check passed"

# --- Optional HTTP probes --------------------------------------------------

http_probe() {
  if ! curl -fsS "${BASE}/healthz" >/dev/null 2>&1; then
    info "no server responding at ${BASE}; skipping HTTP probes"
    return 0
  fi

  info "GET /debug should return 404"
  local code
  code=$(curl -s -o /dev/null -w '%{http_code}' "${BASE}/debug")
  if [[ "${code}" != "404" ]]; then
    fail "expected GET /debug → 404, got ${code}"
  fi
  record "check: GET /debug → ${code}"

  info "GET /admin/debug should be gated"
  code=$(curl -s -o /dev/null -w '%{http_code}' "${BASE}/admin/debug")
  case "${code}" in
    200|401|403)
      record "check: GET /admin/debug → ${code}"
      ;;
    *)
      fail "unexpected status GET /admin/debug → ${code}"
      ;;
  esac

  info "legacy X-ARW-Gate header should be rejected"
  code=$(curl -s -o /dev/null -w '%{http_code}' -H 'X-ARW-Gate: {}' "${BASE}/about")
  if [[ "${code}" != "410" && "${code}" != "401" ]]; then
    fail "expected legacy capsule header rejection (410/401), got ${code}"
  fi
  record "check: legacy capsule header → ${code}"
}

http_probe

info "legacy surface checks passed"
record "status: PASS"
record "completed: $(date -Iseconds)"
