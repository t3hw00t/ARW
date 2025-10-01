#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: trials_guardrails.sh [options]

Apply a guardrail preset for trial rehearsals using the unified server API.

Options:
  --preset NAME        Guardrail preset to apply (default: trial)
  --base URL           Server base URL (default: ${ARW_BASE_URL:-http://127.0.0.1:8091})
  --token TOKEN        Admin bearer token (defaults to ARW_ADMIN_TOKEN)
  --dry-run            Preview only; do not write gating config
  --help               Show this help text

Example:
  ./scripts/trials_guardrails.sh --preset trial --base http://127.0.0.1:8091
USAGE
}

BASE_URL="${ARW_BASE_URL:-http://127.0.0.1:8091}"
TOKEN="${ARW_ADMIN_TOKEN:-}"
PRESET="trial"
DRY_RUN=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --preset)
      PRESET="$2"; shift 2 ;;
    --preset=*)
      PRESET="${1#*=}"; shift ;;
    preset=*)
      PRESET="${1#preset=}"; shift ;;
    --base)
      BASE_URL="$2"; shift 2 ;;
    --base=*)
      BASE_URL="${1#*=}"; shift ;;
    base=*)
      BASE_URL="${1#base=}"; shift ;;
    --token)
      TOKEN="$2"; shift 2 ;;
    --token=*)
      TOKEN="${1#*=}"; shift ;;
    token=*)
      TOKEN="${1#token=}"; shift ;;
    --dry-run)
      DRY_RUN=1; shift ;;
    --help|-h)
      usage; exit 0 ;;
    *)
      printf 'Unknown option: %s\n' "$1" >&2
      usage
      exit 1 ;;
  esac
done

if [[ -z "$PRESET" ]]; then
  printf 'Preset name is required.\n' >&2
  exit 1
fi

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'Command not found: %s\n' "$1" >&2
    exit 1
  fi
}

require_cmd curl

BASE_URL="${BASE_URL%/}"
AUTH_HEADER=()
if [[ -n "$TOKEN" ]]; then
  AUTH_HEADER=( -H "Authorization: Bearer ${TOKEN}" )
fi

dry_flag=$([[ $DRY_RUN -eq 1 ]] && echo true || echo false)
payload=$(printf '{"preset":"%s","dry_run":%s}' "$PRESET" "$dry_flag")

printf '-> Applying guardrail preset "%s" (dry_run=%s)\n' "$PRESET" "$dry_flag"
response=$(curl -sS -w '\n%{http_code}' -X POST "${BASE_URL}/policy/guardrails/apply" \
  -H 'Content-Type: application/json' "${AUTH_HEADER[@]}" -d "$payload")
body=${response%$'\n'*}
status=${response##*$'\n'}

if [[ "$status" != 2* ]]; then
  printf 'Guardrail apply failed (HTTP %s)\n' "$status" >&2
  printf '%s\n' "$body" >&2
  exit 1
fi

printf '%s\n' "$body"
