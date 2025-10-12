#!/usr/bin/env bash
# shellcheck disable=SC2086
set -euo pipefail

usage() {
  cat <<'USAGE'
verify_bundle_signatures.sh [--help]

Environment variables:
  BASE_URL or first positional argument  Base URL for the ARW server (default: http://127.0.0.1:8091)
  ARW_ADMIN_TOKEN                        Admin bearer token (falls back to CLI defaults when unset)
  ADMIN_TOKEN                            Optional alias for ARW_ADMIN_TOKEN
  ARW_CLI_BIN                            Path to arw-cli executable (default: arw-cli on PATH)
  ARW_TIMEOUT_SECS                       Request timeout in seconds (default: 10)

Additional arguments are forwarded to `arw-cli runtime bundles audit`.
USAGE
}

if [[ "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

CLI_BIN="${ARW_CLI_BIN:-arw-cli}"
if ! command -v "$CLI_BIN" >/dev/null 2>&1; then
  echo "error: unable to find '$CLI_BIN'; set ARW_CLI_BIN or place arw-cli on PATH" >&2
  exit 1
fi

BASE="${BASE_URL:-${1:-http://127.0.0.1:8091}}"
if [[ "${1:-}" == "$BASE" ]]; then
  shift
fi

ADMIN="${ARW_ADMIN_TOKEN:-${ADMIN_TOKEN:-}}"
TIMEOUT="${ARW_TIMEOUT_SECS:-10}"

AUDIT_ARGS=(runtime bundles audit --remote --base "$BASE" --require-signed --timeout "$TIMEOUT")
if [[ -n "$ADMIN" ]]; then
  AUDIT_ARGS+=(--admin-token "$ADMIN")
fi

AUDIT_ARGS+=("$@")

exec "$CLI_BIN" "${AUDIT_ARGS[@]}"
