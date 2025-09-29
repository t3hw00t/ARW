#!/usr/bin/env bash
set -euo pipefail

# Create or update a GitHub Release for the TS client tag from package.json
# Requires: gh (GitHub CLI) authenticated with push/release scope
# Usage: scripts/gh_release_ts_client.sh [--draft]

draft=0
if [[ "${1:-}" == "--draft" ]]; then draft=1; fi

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
pkg="$root_dir/clients/typescript/package.json"
chlog="$root_dir/clients/typescript/CHANGELOG.md"
ver=$(jq -r .version "$pkg")
tag="ts-client-v$ver"

notes=$(awk -v ver="$ver" '
  BEGIN {p=0}
  /^##[ ]+([0-9]+\.[0-9]+\.[0-9]+)/ {
    cur=$2; gsub(/^v?/, "", cur);
    if(cur==ver){p=1; next}
    if(p==1){ exit }
  }
  p==1 { print }
' "$chlog")

title="TypeScript client v$ver"
flags=()
if [[ $draft -eq 1 ]]; then flags+=("--draft"); fi

if gh release view "$tag" >/dev/null 2>&1; then
  echo "updating existing release $tag"
  gh release edit "$tag" -t "$title" -n "$notes"
else
  echo "creating release $tag"
  gh release create "$tag" -t "$title" -n "$notes" "${flags[@]}"
fi

