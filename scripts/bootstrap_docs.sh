#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: bash scripts/bootstrap_docs.sh [--upgrade] [--system] [--wheel-dir DIR]

Install the pinned MkDocs/doc generation requirements listed in requirements/docs.txt.
By default packages are installed into the user's site-packages directory.

Options:
  --upgrade   Allow pip to upgrade existing packages to the pinned versions.
  --system    Install into the system interpreter (omit --user). Requires write access.
  --wheel-dir DIR
              Install from a local wheel cache (generated via scripts/build_docs_wheels.sh).
  -h, --help  Show this help message.
EOF
}

allow_upgrade=0
use_system=0
wheel_dir=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --upgrade) allow_upgrade=1; shift ;;
    --system) use_system=1; shift ;;
    --wheel-dir)
      wheel_dir="$2"
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) echo "[bootstrap-docs] Unknown option: $1" >&2; usage; exit 2 ;;
  esac
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REQ_FILE="$ROOT_DIR/requirements/docs.txt"

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

pip_cmd=("$PYTHON_BIN" -m pip install)
if [[ $allow_upgrade -eq 1 ]]; then
  pip_cmd+=(--upgrade)
fi
if [[ $use_system -eq 0 ]]; then
  pip_cmd+=(--user)
fi
if [[ -n "$wheel_dir" ]]; then
  pip_cmd+=(--no-index --find-links "$wheel_dir")
fi
pip_cmd+=(--require-hashes -r "$REQ_FILE")

echo "[bootstrap-docs] Installing MkDocs/doc requirements via pip"
PIP_BREAK_SYSTEM_PACKAGES=1 "${pip_cmd[@]}"
echo "[bootstrap-docs] Done."
