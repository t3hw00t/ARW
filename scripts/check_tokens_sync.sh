#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
SRC_CSS="$ROOT_DIR/assets/design/tokens.css"
SRC_JSON="$ROOT_DIR/assets/design/tokens.json"

DEST_DOCS_CSS="$ROOT_DIR/docs/css/tokens.css"
DEST_DOCS_JSON="$ROOT_DIR/docs/design/tokens.json"
DEST_APP_CSS="$ROOT_DIR/apps/arw-launcher/src-tauri/ui/tokens.css"

fail=0

cmp -s "$SRC_CSS" "$DEST_DOCS_CSS" || { echo "docs/css/tokens.css out of sync"; fail=1; }
cmp -s "$SRC_CSS" "$DEST_APP_CSS"  || { echo "apps/arw-launcher/src-tauri/ui/tokens.css out of sync"; fail=1; }
cmp -s "$SRC_JSON" "$DEST_DOCS_JSON" || { echo "docs/design/tokens.json out of sync"; fail=1; }

if (( fail != 0 )); then
  echo "Run: just tokens-sync" >&2
  exit 2
fi

echo "Tokens are in sync."

