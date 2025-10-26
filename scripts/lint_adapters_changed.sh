#!/usr/bin/env bash
set -euo pipefail

# Lint only changed adapter manifests compared to a base ref.
# Usage: BASE=origin/main bash scripts/lint_adapters_changed.sh

BASE_REF="${BASE:-origin/main}"

if ! git rev-parse --verify "$BASE_REF" >/dev/null 2>&1; then
  echo "[adapters-lint-changed] base ref not found: $BASE_REF" >&2
  exit 2
fi

changed=$(git diff --name-only "$BASE_REF" -- adapters | grep -E '\\.(json|toml)$' || true)
if [[ -z "$changed" ]]; then
  echo "[adapters-lint-changed] no changed manifests under adapters/ vs $BASE_REF"
  exit 0
fi

echo "[adapters-lint-changed] files:"$'\n'"   ,  ${changed//$'\n'/$'\n   ,  '}"

export ADAPTERS_FILES="$changed"
bash "$(dirname "$0")/lint_adapters.sh"

