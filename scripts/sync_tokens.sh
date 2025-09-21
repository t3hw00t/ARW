#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
SRC_TOKENS_CSS="$ROOT_DIR/assets/design/tokens.css"
SRC_TOKENS_JSON="$ROOT_DIR/assets/design/tokens.json"
SRC_UI_KIT_CSS="$ROOT_DIR/assets/design/ui-kit.css"

DEST_DOCS_TOKENS_CSS="$ROOT_DIR/docs/css/tokens.css"
DEST_DOCS_TOKENS_JSON="$ROOT_DIR/docs/design/tokens.json"
DEST_LAUNCHER_TOKENS_CSS="$ROOT_DIR/apps/arw-launcher/src-tauri/ui/tokens.css"
DEST_LAUNCHER_UI_KIT_CSS="$ROOT_DIR/apps/arw-launcher/src-tauri/ui/ui-kit.css"
DEST_SERVER_TOKENS_CSS="$ROOT_DIR/apps/arw-server/assets/ui/tokens.css"
DEST_SERVER_UI_KIT_CSS="$ROOT_DIR/apps/arw-server/assets/ui/ui-kit.css"

for f in "$SRC_TOKENS_CSS" "$SRC_TOKENS_JSON" "$SRC_UI_KIT_CSS"; do
  [[ -f "$f" ]] || { echo "missing source: $f" >&2; exit 1; }
done

install -d \
  "$(dirname "$DEST_DOCS_TOKENS_CSS")" \
  "$(dirname "$DEST_DOCS_TOKENS_JSON")" \
  "$(dirname "$DEST_LAUNCHER_TOKENS_CSS")" \
  "$(dirname "$DEST_LAUNCHER_UI_KIT_CSS")" \
  "$(dirname "$DEST_SERVER_TOKENS_CSS")" \
  "$(dirname "$DEST_SERVER_UI_KIT_CSS")"

cp -f "$SRC_TOKENS_CSS" "$DEST_DOCS_TOKENS_CSS"
cp -f "$SRC_TOKENS_CSS" "$DEST_LAUNCHER_TOKENS_CSS"
cp -f "$SRC_TOKENS_CSS" "$DEST_SERVER_TOKENS_CSS"
cp -f "$SRC_TOKENS_JSON" "$DEST_DOCS_TOKENS_JSON"
cp -f "$SRC_UI_KIT_CSS" "$DEST_LAUNCHER_UI_KIT_CSS"
cp -f "$SRC_UI_KIT_CSS" "$DEST_SERVER_UI_KIT_CSS"

echo "Design tokens + UI kit synced to:"
echo " - $DEST_DOCS_TOKENS_CSS"
echo " - $DEST_DOCS_TOKENS_JSON"
echo " - $DEST_LAUNCHER_TOKENS_CSS"
echo " - $DEST_LAUNCHER_UI_KIT_CSS"
echo " - $DEST_SERVER_TOKENS_CSS"
echo " - $DEST_SERVER_UI_KIT_CSS"
