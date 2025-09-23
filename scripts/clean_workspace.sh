#!/usr/bin/env bash
set -euo pipefail

# Clean workspace artifacts produced by cargo, docs, and tooling.
# Usage: clean_workspace.sh [--venv]
#   --venv   additionally removes the local Python virtual environment (.venv).

remove_venv=false

while (($#)); do
  case "$1" in
    --venv)
      remove_venv=true
      ;;
    -h|--help)
      cat <<'HELP'
Usage: clean_workspace.sh [options]

Options:
  --venv   remove the local Python virtual environment (.venv)
  -h      show this help text
HELP
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
  shift
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Clean cargo artifacts first.
if command -v cargo >/dev/null 2>&1; then
  cargo clean
fi

# Remove generated directories that are not covered by cargo clean.
rm -rf \
  .install.log \
  site \
  dist \
  apps/arw-launcher/src-tauri/bin \
  apps/arw-launcher/src-tauri/gen \
  apps/arw-server/state \
  scripts/__pycache__ \
  target/nextest \
  target/tmp

# Remove any stray __pycache__ directories across the repository.
find . -type d -name '__pycache__' -prune -exec rm -rf {} +

if [[ "$remove_venv" == true ]]; then
  rm -rf .venv
fi

exit 0
