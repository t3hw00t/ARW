#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
CFG="$ROOT_DIR/assets/design/style-dictionary.config.cjs"

if ! command -v npx >/dev/null 2>&1; then
  echo "npx not found; skipping Style Dictionary build (optional)." >&2
  exit 0
fi

echo "Building tokens with Style Dictionary..."
npx --yes style-dictionary build --config "$CFG"

echo "Done. Outputs in assets/design/generated/"

