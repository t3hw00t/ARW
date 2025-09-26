#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: autonomy_rollback.sh --lane LANE [options]

Options:
  --lane LANE             Required. Autonomy lane identifier (e.g. trial-g4-autonomy).
  --project PROJECT       Project identifier to restore. Discovered automatically when possible.
  --runtime RUNTIME       Runtime identifier to restore via runtime manager (optional).
  --snapshot SNAPSHOT     Explicit project snapshot id. Defaults to latest available when omitted.
  --guardrails PRESET     Named guardrail preset to reapply after restore.
  --base URL              Base URL (default: http://127.0.0.1:8091).
  --token TOKEN           Admin token. Defaults to ARW_ADMIN_TOKEN environment variable.
  --dry-run               Print the planned operations without performing mutating calls.
  --continue-on-error     Continue even if a step fails (default: continue).
  --help                  Show this help text.

Environment:
  ARW_ADMIN_TOKEN         Supplies admin token if --token omitted.
  ARW_BASE_URL            Supplies default base URL if --base omitted.

The script issues admin API requests to pause the lane, flush jobs, restore
project state, reapply guardrails, and resume guided mode. When endpoints are
missing or return errors the script surfaces detailed guidance and keeps going
so operators can finish manually.
USAGE
}

log() { printf '%s %s\n' '->' "$1"; }
warn() { printf '%s %s\n' 'WARN:' "$1" >&2; }
err() { printf '%s %s\n' 'ERROR:' "$1" >&2; }

require_cmd() {
  local cmd="$1"; local msg="$2"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    err "${msg}"
    exit 1
  fi
}

BASE_URL="${ARW_BASE_URL:-http://127.0.0.1:8091}"
LANE=""
PROJECT=""
RUNTIME=""
SNAPSHOT=""
GUARDRAIL_PRESET=""
TOKEN="${ARW_ADMIN_TOKEN:-}"
DRY_RUN=0
CONTINUE_ON_ERROR=1

while [[ $# -gt 0 ]]; do
  case "$1" in
    --lane)
      LANE="$2"; shift 2 ;;
    --lane=*)
      LANE="${1#*=}"; shift ;;
    lane=*)
      LANE="${1#lane=}"; shift ;;
    --project)
      PROJECT="$2"; shift 2 ;;
    --project=*)
      PROJECT="${1#*=}"; shift ;;
    project=*)
      PROJECT="${1#project=}"; shift ;;
    --runtime)
      RUNTIME="$2"; shift 2 ;;
    --runtime=*)
      RUNTIME="${1#*=}"; shift ;;
    runtime=*)
      RUNTIME="${1#runtime=}"; shift ;;
    --snapshot)
      SNAPSHOT="$2"; shift 2 ;;
    --snapshot=*)
      SNAPSHOT="${1#*=}"; shift ;;
    snapshot=*)
      SNAPSHOT="${1#snapshot=}"; shift ;;
    --guardrails)
      GUARDRAIL_PRESET="$2"; shift 2 ;;
    --guardrails=*)
      GUARDRAIL_PRESET="${1#*=}"; shift ;;
    guardrails=*)
      GUARDRAIL_PRESET="${1#guardrails=}"; shift ;;
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
    --continue-on-error)
      CONTINUE_ON_ERROR=1; shift ;;
    --fail-fast)
      CONTINUE_ON_ERROR=0; shift ;;
    --help|-h)
      usage; exit 0 ;;
    *)
      err "Unknown argument: $1"; usage; exit 1 ;;
  esac
done

if [[ -z "$LANE" ]]; then
  err "--lane is required"
  usage
  exit 1
fi

require_cmd curl "curl is required to contact the ARW server"
require_cmd jq "jq is required to parse JSON responses"

BASE_URL="${BASE_URL%/}"
AUTH_HEADER=()
if [[ -n "$TOKEN" ]]; then
  AUTH_HEADER=( -H "Authorization: Bearer ${TOKEN}" )
fi

API_STATUS=""
API_BODY=""

api_call() {
  local method="$1"; shift
  local path="$1"; shift
  local data="${1:-}"
  local tmp
  tmp=$(mktemp)
  local curl_args=( -sS -o "$tmp" -w '%{http_code}' -X "$method" )
  curl_args+=("${BASE_URL}${path}")
  if [[ -n "$data" ]]; then
    curl_args+=( -H "Content-Type: application/json" -d "$data" )
  fi
  if [[ ${#AUTH_HEADER[@]} -gt 0 ]]; then
    curl_args+=("${AUTH_HEADER[@]}")
  fi
  set +e
  local status
  status=$(curl "${curl_args[@]}")
  local code=$?
  set -e
  API_STATUS="${status}"
  if (( code != 0 )); then
    rm -f "$tmp"
    return 1
  fi
  API_BODY="$(<"$tmp")"
  rm -f "$tmp"
  return 0
}

run_step() {
  local name="$1"; shift
  local fn="$1"; shift
  log "${name}"
  if ! "$fn" "$@"; then
    if (( CONTINUE_ON_ERROR )); then
      warn "${name} failed; continuing"
      return 0
    else
      err "${name} failed; aborting"
      return 1
    fi
  fi
  return 0
}

pause_lane() {
  if (( DRY_RUN )); then
    log "DRY RUN: would POST /admin/autonomy/${LANE}/pause"
    return 0
  fi
  if api_call POST "/admin/autonomy/${LANE}/pause"; then
    if [[ "$API_STATUS" =~ ^20 ]]; then
      log "Lane paused"
      return 0
    fi
  fi
  warn "Pause lane returned status ${API_STATUS:-unknown}."
  warn "Manual fallback: POST ${BASE_URL}/admin/autonomy/${LANE}/pause"
  return 1
}

flush_jobs() {
  if (( DRY_RUN )); then
    log "DRY RUN: would DELETE /admin/autonomy/${LANE}/jobs?state=in_flight"
    log "DRY RUN: would DELETE /admin/autonomy/${LANE}/jobs"
    return 0
  fi
  local ok=0
  if api_call DELETE "/admin/autonomy/${LANE}/jobs?state=in_flight"; then
    if [[ "$API_STATUS" =~ ^20 ]]; then
      ok=1
    fi
  fi
  if api_call DELETE "/admin/autonomy/${LANE}/jobs"; then
    if [[ "$API_STATUS" =~ ^20 ]]; then
      ok=1
    fi
  fi
  if (( ok )); then
    log "Lane queues flushed"
    return 0
  fi
  warn "Failed to flush autonomy jobs (status ${API_STATUS:-unknown})."
  warn "Manual fallback: DELETE ${BASE_URL}/admin/autonomy/${LANE}/jobs"
  return 1
}

lane_metadata() {
  if (( DRY_RUN )); then
    return 0
  fi
  if api_call GET "/state/autonomy/lanes/${LANE}"; then
    if [[ "$API_STATUS" =~ ^20 ]]; then
      [[ -z "$PROJECT" ]] && PROJECT="$(echo "$API_BODY" | jq -r '(.project.id // .project_id // .scope.project // .project // empty)')"
      [[ -z "$RUNTIME" ]] && RUNTIME="$(echo "$API_BODY" | jq -r '(.runtime.id // .runtime_id // .runtimes[0].id // empty)')"
      [[ -z "$GUARDRAIL_PRESET" ]] && GUARDRAIL_PRESET="$(echo "$API_BODY" | jq -r '(.guardrail_preset // .presets.guardrails // empty)')"
      if [[ -z "$SNAPSHOT" ]]; then
        SNAPSHOT="$(echo "$API_BODY" | jq -r '(.last_snapshot.id // .snapshots.latest // empty)')"
      fi
      return 0
    fi
  fi
  warn "Lane metadata endpoint unavailable (status ${API_STATUS:-unknown})."
  warn "Provide --project/--runtime/--snapshot manually if automation needs them."
  return 1
}

latest_snapshot() {
  if (( DRY_RUN )); then
    return 0
  fi
  if [[ -n "$SNAPSHOT" ]]; then
    return 0
  fi
  if [[ -z "$PROJECT" ]]; then
    warn "Project id unknown; cannot auto-select snapshot"
    return 1
  fi
  if api_call GET "/admin/projects/${PROJECT}/snapshots?limit=1"; then
    if [[ "$API_STATUS" =~ ^20 ]]; then
      SNAPSHOT="$(echo "$API_BODY" | jq -r '.items[0].id // .snapshots[0].id // empty')"
      if [[ -n "$SNAPSHOT" && "$SNAPSHOT" != "null" ]]; then
        log "Selected snapshot ${SNAPSHOT}"
        return 0
      fi
    fi
  fi
  warn "Unable to determine latest snapshot automatically (status ${API_STATUS:-unknown})."
  warn "Provide --snapshot SNAPSHOT_ID to restore explicitly."
  return 1
}

restore_project() {
  if [[ -z "$PROJECT" ]]; then
    warn "No project id available; skipping project restore"
    return 1
  fi
  if [[ -z "$SNAPSHOT" ]]; then
    warn "No snapshot id available; skipping project restore"
    return 1
  fi
  if (( DRY_RUN )); then
    log "DRY RUN: would POST /admin/projects/${PROJECT}/restore snapshot=${SNAPSHOT}"
    return 0
  fi
  local payload
  payload="$(jq -nc --arg snap "$SNAPSHOT" '{snapshot_id: $snap}')"
  if api_call POST "/admin/projects/${PROJECT}/restore" "$payload"; then
    if [[ "$API_STATUS" =~ ^20 ]]; then
      log "Project ${PROJECT} restored to snapshot ${SNAPSHOT}"
      return 0
    fi
  fi
  warn "Project restore failed (status ${API_STATUS:-unknown})."
  warn "Manual fallback: POST ${BASE_URL}/admin/projects/${PROJECT}/restore with snapshot_id=${SNAPSHOT}"
  return 1
}

restore_runtime() {
  if [[ -z "$RUNTIME" ]]; then
    warn "No runtime id provided; skipping runtime restore"
    return 1
  fi
  if (( DRY_RUN )); then
    log "DRY RUN: would POST /admin/runtimes/${RUNTIME}/restore"
    return 0
  fi
  if api_call POST "/admin/runtimes/${RUNTIME}/restore"; then
    if [[ "$API_STATUS" =~ ^20 ]]; then
      log "Runtime ${RUNTIME} restore requested"
      return 0
    fi
  fi
  warn "Runtime restore call failed (status ${API_STATUS:-unknown})."
  warn "Manual fallback: restart runtime ${RUNTIME} via launcher Runtime Manager"
  return 1
}

reapply_guardrails() {
  if [[ -z "$GUARDRAIL_PRESET" ]]; then
    warn "No guardrail preset provided; skipping guardrail reapply"
    return 1
  fi
  if (( DRY_RUN )); then
    log "DRY RUN: would PATCH /admin/gating preset=${GUARDRAIL_PRESET}"
    return 0
  fi
  local payload
  payload="$(jq -nc --arg preset "$GUARDRAIL_PRESET" '{preset: $preset}')"
  if api_call PATCH "/admin/gating" "$payload"; then
    if [[ "$API_STATUS" =~ ^20 ]]; then
      log "Guardrail preset ${GUARDRAIL_PRESET} re-applied"
      return 0
    fi
  fi
  warn "Guardrail preset apply failed (status ${API_STATUS:-unknown})."
  warn "Manual fallback: PATCH ${BASE_URL}/admin/gating with preset ${GUARDRAIL_PRESET}"
  return 1
}

summarize_context() {
  log "Lane:        ${LANE}"
  log "Project:     ${PROJECT:-<unknown>}"
  log "Runtime:     ${RUNTIME:-<unknown>}"
  log "Snapshot:    ${SNAPSHOT:-<auto>}"
  log "Guardrails:  ${GUARDRAIL_PRESET:-<none>}"
  log "Base URL:    ${BASE_URL}"
  if (( DRY_RUN )); then
    log "Mode:        DRY RUN"
  fi
}

summarize_context

run_step "Discover lane metadata" lane_metadata || true
run_step "Select snapshot" latest_snapshot || true
run_step "Pause lane" pause_lane
run_step "Flush jobs" flush_jobs
run_step "Restore project" restore_project || true
run_step "Restore runtime" restore_runtime || true
run_step "Reapply guardrails" reapply_guardrails || true

log "Autonomy rollback helper finished"
if [[ -z "$PROJECT" ]]; then
  warn "Project id unresolved. Ensure manual restore completed."
fi
if [[ -z "$SNAPSHOT" ]]; then
  warn "Snapshot id unresolved. Provide --snapshot next time."
fi
if [[ -z "$RUNTIME" ]]; then
  warn "Runtime id unresolved or restore skipped."
fi
