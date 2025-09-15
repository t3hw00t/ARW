#!/usr/bin/env bash
set -euo pipefail

BASE_URL=${1:-http://127.0.0.1:8000}
OUT_DIR=${2:-a11y-reports}
mkdir -p "$OUT_DIR"

PAGES=(
  "/"
  "/developer/index/"
  "/developer/design_theme/"
  "/developer/ui_kit/"
  "/developer/standards/"
  "/guide/launcher/"
  "/guide/workflow_views/"
  "/architecture/events_vocabulary/"
  "/reference/feature_matrix/"
)

if ! command -v npx >/dev/null 2>&1; then
  echo "npx not found; cannot run axe checks" >&2
  exit 0
fi

echo "Running axe on ${#PAGES[@]} pages..."
for path in "${PAGES[@]}"; do
  url="$BASE_URL$path"
  slug=$(echo "$path" | tr '/ ' '__')
  npx --yes @axe-core/cli -q -f json -o "$OUT_DIR/axe$slug.json" "$url" || true
done

echo "Axe reports written to $OUT_DIR"

