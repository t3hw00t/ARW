#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

usage() {
  cat <<'USAGE'
Usage: scripts/maintenance.sh [options] [tasks...]

Without arguments the script runs the default maintenance cycle:
  clean, prune-logs, prune-tokens, docs, cargo-check, audit-summary, pointer-migrate

Tasks:
  clean           Remove build artifacts, dist/, site/, launcher cache, tmp
  prune-logs      Trim .arw/logs and target/logs to last 7 days (configurable)
  prune-tokens    Delete any .arw/last_*.txt residual token files
  docs            Remove site/ and re-run doc stamps (stamp_docs_updated)
  cargo-check     Run cargo check --workspace to ensure sources still build
  audit-summary   Run scripts/audit.sh --summary (skips interactive prompts)
  format          Invoke cargo fmt && npm/just lint hooks if available
  hooks           Reinstall git hooks (scripts/hooks/install_hooks.sh)
  vacuum          Vacuum sqlite journals/state (apps/arw-server/state)
  pointer-migrate Canonicalise pointer tokens in the state directory (uses scripts/migrate_pointer_tokens.py)
  help            Show this help

Options:
  --dry-run       Show actions without executing destructive commands
  --keep-logs N   Retain N days of logs (default: 7)
  --state-dir DIR Override the state directory (default: apps/arw-server/state)
  --pointer-consent LEVEL Default consent to apply when missing (private|shared|public; default private)
USAGE
}

DRY_RUN=0
KEEP_LOGS_DAYS=7
STATE_DIR_OVERRIDE=""
POINTER_CONSENT="private"
TASKS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run) DRY_RUN=1; shift ;;
    --keep-logs) KEEP_LOGS_DAYS="${2:-}"; shift 2 ;;
    --state-dir) STATE_DIR_OVERRIDE="${2:-}"; shift 2 ;;
    --pointer-consent) POINTER_CONSENT="${2:-}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    clean|prune-logs|prune-tokens|docs|cargo-check|audit-summary|format|hooks|vacuum|pointer-migrate|help)
      TASKS+=("$1"); shift ;;
    *)
      echo "Unknown option/task: $1" >&2
      usage; exit 1 ;;
  esac
done

if [[ ${#TASKS[@]} -eq 0 ]]; then
  TASKS=(clean prune-logs prune-tokens docs cargo-check audit-summary pointer-migrate)
fi

run() {
  if [[ $DRY_RUN -eq 1 ]]; then
    echo "[maintenance] dry-run: $*"
  else
    echo "[maintenance] running: $*"
    "$@"
  fi
}

dry_rm() {
  local target="$1"
  if [[ $DRY_RUN -eq 1 ]]; then
    echo "[maintenance] dry-run: rm -rf $target"
  else
    rm -rf "$target"
  fi
}

task_clean() {
  dry_rm "$REPO_ROOT/target"
  dry_rm "$REPO_ROOT/dist"
  dry_rm "$REPO_ROOT/site"
  dry_rm "$REPO_ROOT/apps/arw-launcher/src-tauri/bin"
  dry_rm "$REPO_ROOT/apps/arw-launcher/src-tauri/gen"
  dry_rm "$REPO_ROOT/apps/arw-server/state/tmp"
  dry_rm "$REPO_ROOT/target/tmp"
  dry_rm "$REPO_ROOT/target/nextest"
  find "$REPO_ROOT" -type d -name '__pycache__' -prune -print0 | while IFS= read -r -d '' p; do dry_rm "$p"; done
}

task_prune_logs() {
  local limit_days="$KEEP_LOGS_DAYS"
  local cutoff
  cutoff=$(date -d "${limit_days} days ago" +%s 2>/dev/null || date -v -"${limit_days}"d +%s)
  prune_dir() {
    local dir="$1"
    [[ -d "$dir" ]] || return 0
    find "$dir" -type f -print0 | while IFS= read -r -d '' file; do
      local mtime
      mtime=$(stat -c %Y "$file" 2>/dev/null || stat -f %m "$file" 2>/dev/null || echo 0)
      if (( mtime < cutoff )); then
        if [[ $DRY_RUN -eq 1 ]]; then
          echo "[maintenance] dry-run: delete old log $file"
        else
          rm -f "$file"
          echo "[maintenance] deleted log $file"
        fi
      fi
    done
  }
  prune_dir "$REPO_ROOT/.arw/logs"
  prune_dir "$REPO_ROOT/target/logs"
}

task_prune_tokens() {
  find "$REPO_ROOT/.arw" -maxdepth 1 -type f -name 'last_*token*.txt' -print0 2>/dev/null | while IFS= read -r -d '' file; do
    if [[ $DRY_RUN -eq 1 ]]; then
      echo "[maintenance] dry-run: delete token file $file"
    else
      shred -u "$file" 2>/dev/null || rm -f "$file"
      echo "[maintenance] deleted token file $file"
    fi
  done
}

task_docs() {
  dry_rm "$REPO_ROOT/site"
  if command -v python3 >/dev/null 2>&1; then
    run python3 "$SCRIPT_DIR/stamp_docs_updated.py"
  fi
}

task_cargo_check() {
  if command -v cargo >/dev/null 2>&1; then
    run cargo check --workspace
  else
    echo "[maintenance] cargo not available; skipping cargo-check"
  fi
}

task_audit_summary() {
  if [[ -x "$SCRIPT_DIR/audit.sh" ]]; then
    run bash "$SCRIPT_DIR/audit.sh" --summary
  else
    echo "[maintenance] audit.sh not executable; skipping"
  fi
}

task_format() {
  if command -v cargo >/dev/null 2>&1; then run cargo fmt --all; fi
  if command -v npm >/dev/null 2>&1 && [[ -f package.json ]]; then run npm run lint -- --fix || true; fi
  if [[ -x "$SCRIPT_DIR/hooks/install_hooks.sh" ]]; then run bash "$SCRIPT_DIR/hooks/install_hooks.sh"; fi
}

task_hooks() {
  if [[ -x "$SCRIPT_DIR/hooks/install_hooks.sh" ]]; then
    run bash "$SCRIPT_DIR/hooks/install_hooks.sh"
  fi
}

task_vacuum() {
  local state_dir="${STATE_DIR_OVERRIDE:-$REPO_ROOT/apps/arw-server/state}"
  if [[ -d "$state_dir" ]]; then
    find "$state_dir" -type f -name '*.sqlite' -print0 | while IFS= read -r -d '' db; do
      if command -v sqlite3 >/dev/null 2>&1; then
        if [[ $DRY_RUN -eq 1 ]]; then
          echo "[maintenance] dry-run: vacuum $db"
        else
          sqlite3 "$db" 'VACUUM;'
        fi
      fi
    done
  fi
}

task_pointer_migrate() {
  local state_dir="${STATE_DIR_OVERRIDE:-$REPO_ROOT/apps/arw-server/state}"
  if [[ ! -d "$state_dir" ]]; then
    echo "[maintenance] pointer-migrate: state dir $state_dir not found; skipping"
    return 0
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    echo "[maintenance] python3 missing; skipping pointer-migrate"
    return 0
  fi
  local cmd=(python3 "$SCRIPT_DIR/migrate_pointer_tokens.py" --state-dir "$state_dir" --default-consent "$POINTER_CONSENT")
  if [[ $DRY_RUN -eq 1 ]]; then
    cmd+=(--dry-run)
    echo "[maintenance] dry-run: ${cmd[*]}"
    if ! "${cmd[@]}"; then
      echo "[maintenance] pointer-migrate dry-run failed; ensure state is accessible" >&2
    fi
  else
    if ! run "${cmd[@]}"; then
      echo "[maintenance] pointer-migrate failed; the state directory may be locked (stop the server first)" >&2
    fi
  fi
}

for task in "${TASKS[@]}"; do
  case "$task" in
    clean) task_clean ;;
    prune-logs) task_prune_logs ;;
    prune-tokens) task_prune_tokens ;;
    docs) task_docs ;;
    cargo-check) task_cargo_check ;;
    audit-summary) task_audit_summary ;;
    format) task_format ;;
    hooks) task_hooks ;;
    vacuum) task_vacuum ;;
    pointer-migrate) task_pointer_migrate ;;
    help) usage ;;
    *) echo "[maintenance] unknown task $task" ;;
  esac
done

echo "[maintenance] completed"
