#!/usr/bin/env bash
set -euo pipefail

# Install llmlingua with a CPU-only torch wheel by default.
# Override LLMLINGUA_VENV to pick a different venv, or TORCH_INDEX_URL to force a different torch index.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VENV="${LLMLINGUA_VENV:-$REPO_ROOT/.venv}"
PYTHON="$VENV/bin/python"
TORCH_INDEX_URL="${TORCH_INDEX_URL:-https://download.pytorch.org/whl/cpu}"

echo "[llmlingua-setup] venv: $VENV"
if [[ ! -x "$PYTHON" ]]; then
  echo "[llmlingua-setup] creating venv..."
  python3 -m venv "$VENV"
fi

echo "[llmlingua-setup] upgrading pip..."
"$PYTHON" -m pip install --upgrade pip

echo "[llmlingua-setup] installing torch (CPU) from $TORCH_INDEX_URL..."
"$PYTHON" -m pip install --force-reinstall --index-url "$TORCH_INDEX_URL" "torch==2.9.1+cpu"

echo "[llmlingua-setup] installing llmlingua..."
"$PYTHON" -m pip install llmlingua

cat <<EOF
[llmlingua-setup] done.
  Interpreter: $PYTHON
  Export LLMLINGUA_PYTHON="$PYTHON" when running arw-server (scripts/dev.sh sets this automatically if unset).
EOF
