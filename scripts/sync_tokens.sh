#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
SRC_CSS="$ROOT_DIR/assets/design/tokens.css"
SRC_JSON="$ROOT_DIR/assets/design/tokens.json"

DEST_DOCS_CSS="$ROOT_DIR/docs/css/tokens.css"
DEST_DOCS_JSON="$ROOT_DIR/docs/design/tokens.json"
DEST_APP_CSS="$ROOT_DIR/apps/arw-launcher/src-tauri/ui/tokens.css"

for f in "$SRC_CSS" "$SRC_JSON"; do
  [[ -f "$f" ]] || { echo "missing source: $f" >&2; exit 1; }
done

install -d "$(dirname "$DEST_DOCS_CSS")" "$(dirname "$DEST_DOCS_JSON")" "$(dirname "$DEST_APP_CSS")"

cp -f "$SRC_CSS"  "$DEST_DOCS_CSS"
cp -f "$SRC_CSS"  "$DEST_APP_CSS"
cp -f "$SRC_JSON" "$DEST_DOCS_JSON"

echo "Tokens synced to:"
echo " - $DEST_DOCS_CSS"
echo " - $DEST_APP_CSS"
echo " - $DEST_DOCS_JSON"

