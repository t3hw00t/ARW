#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: bash scripts/bootstrap_docs.sh [--upgrade] [--system] [--wheel-dir DIR] [--venv DIR]

Install the pinned MkDocs/doc generation requirements listed in requirements/docs.txt.
By default packages are installed into a managed virtual environment at .venv/docs.

Options:
  --upgrade     Allow pip to upgrade existing packages to the pinned versions.
  --system      Install into the system interpreter (omit --venv). Requires write access.
  --wheel-dir DIR
                Install from a local wheel cache (generated via scripts/build_docs_wheels.sh).
  --venv DIR    Path to the virtual environment to hydrate (defaults to .venv/docs).
  -h, --help    Show this help message.
EOF
}

allow_upgrade=0
use_system=0
wheel_dir=""
venv_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --upgrade) allow_upgrade=1; shift ;;
    --system) use_system=1; shift ;;
    --wheel-dir)
      wheel_dir="$2"
      shift 2
      ;;
    --venv)
      venv_dir="$2"
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[bootstrap-docs] Unknown option: $1" >&2; usage; exit 2 ;;
  esac
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REQ_FILE="$ROOT_DIR/requirements/docs.txt"
DEFAULT_VENV="$ROOT_DIR/.venv/docs"

if [[ $use_system -eq 0 ]]; then
  DOCS_VENV="${venv_dir:-$DEFAULT_VENV}"
else
  DOCS_VENV=""
fi

if [[ ! -f "$REQ_FILE" ]]; then
  echo "[bootstrap-docs] requirements/docs.txt not found" >&2
  exit 1
fi

if [[ -n "$wheel_dir" ]]; then
  if [[ ! -d "$wheel_dir" ]]; then
    echo "[bootstrap-docs] wheel cache directory '$wheel_dir' not found" >&2
    exit 1
  fi
fi

if [[ $use_system -eq 0 ]]; then
  if [[ -z "$DOCS_VENV" ]]; then
    echo "[bootstrap-docs] internal error: DOCS_VENV not set" >&2
    exit 1
  fi
  if [[ ! -d "$DOCS_VENV" ]]; then
    echo "[bootstrap-docs] Creating docs venv at $DOCS_VENV"
    python3 -m venv "$DOCS_VENV"
  fi
  PYTHON_BIN="$DOCS_VENV/bin/python"
else
  PYTHON_BIN="$(command -v python3 || command -v python || true)"
  if [[ -z "$PYTHON_BIN" ]]; then
    echo "[bootstrap-docs] python3/python not found in PATH" >&2
    exit 1
  fi
  if ! "$PYTHON_BIN" -m pip --version >/dev/null 2>&1; then
    echo "[bootstrap-docs] pip not detected; attempting ensurepip bootstrap"
    if ! "$PYTHON_BIN" -m ensurepip --upgrade --default-pip >/dev/null 2>&1; then
      echo "[bootstrap-docs] ensurepip failed. Install pip manually and rerun." >&2
      exit 1
    fi
  fi
fi

pip_cmd=("$PYTHON_BIN" -m pip install)
if [[ $allow_upgrade -eq 1 ]]; then
  pip_cmd+=(--upgrade)
fi
if [[ -n "$wheel_dir" ]]; then
  pip_cmd+=(--no-index --find-links "$wheel_dir")
fi
pip_cmd+=(--require-hashes -r "$REQ_FILE")

echo "[bootstrap-docs] Installing MkDocs/doc requirements via pip (${PYTHON_BIN})"
"${pip_cmd[@]}"

if [[ -n "$DOCS_VENV" ]]; then
  echo "[bootstrap-docs] Docs venv hydrated: $DOCS_VENV"
fi
echo "[bootstrap-docs] Done."
