#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

log() { printf '-> %s\n' "$1"; }
warn() { printf 'WARN: %s\n' "$1" >&2; }

run_just() {
  local target="$1"
  local label="$2"
  if ! command -v just >/dev/null 2>&1; then
    warn "Skipping ${label}: 'just' not found."
    return
  fi
  if just --list 2>/dev/null | grep -E "^[[:space:]]+${target}(\\s|$)" >/dev/null; then
    log "Running ${label} (${target})"
    just "$target"
  else
    warn "Skipping ${label}: just target '${target}' not defined."
  fi
}

log "Trial preflight starting"
run_just "triad-smoke" "kernel triad smoke check"
run_just "context-ci" "context telemetry checks"

if [ -x "scripts/check_legacy_surface.sh" ]; then
  log "Running legacy surface check"
  bash scripts/check_legacy_surface.sh || warn "Legacy surface check reported issues"
else
  warn "Skipping legacy surface check: script missing"
fi

log "Trial preflight complete"
