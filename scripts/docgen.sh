#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PYTHON_BIN="${PYTHON:-${PYTHON3:-}}"

if [[ -z "${PYTHON_BIN}" ]]; then
  if command -v python3 >/dev/null 2>&1; then
    PYTHON_BIN="$(command -v python3)"
  elif command -v python >/dev/null 2>&1; then
    PYTHON_BIN="$(command -v python)"
  fi
fi

if [[ -z "${PYTHON_BIN}" ]]; then
  echo "[docgen] warning: python not found; skipping doc generation." >&2
  exit 0
fi

exec "$PYTHON_BIN" "$SCRIPT_DIR/docgen_core.py" "$@"
