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

fail=0

cmp -s "$SRC_TOKENS_CSS" "$DEST_DOCS_TOKENS_CSS" || { echo "docs/css/tokens.css out of sync"; fail=1; }
cmp -s "$SRC_TOKENS_CSS" "$DEST_LAUNCHER_TOKENS_CSS" || { echo "apps/arw-launcher/src-tauri/ui/tokens.css out of sync"; fail=1; }
cmp -s "$SRC_TOKENS_CSS" "$DEST_SERVER_TOKENS_CSS" || { echo "apps/arw-server/assets/ui/tokens.css out of sync"; fail=1; }
cmp -s "$SRC_TOKENS_JSON" "$DEST_DOCS_TOKENS_JSON" || { echo "docs/design/tokens.json out of sync"; fail=1; }
cmp -s "$SRC_UI_KIT_CSS" "$DEST_LAUNCHER_UI_KIT_CSS" || { echo "apps/arw-launcher/src-tauri/ui/ui-kit.css out of sync"; fail=1; }
cmp -s "$SRC_UI_KIT_CSS" "$DEST_SERVER_UI_KIT_CSS" || { echo "apps/arw-server/assets/ui/ui-kit.css out of sync"; fail=1; }

if (( fail != 0 )); then
  echo "Run: just tokens-sync" >&2
  exit 2
fi

echo "Design tokens and UI kit are in sync."
