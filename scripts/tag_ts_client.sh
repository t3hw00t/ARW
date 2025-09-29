#!/usr/bin/env bash
set -euo pipefail

# Tag the TS client with the version in clients/typescript/package.json
# Usage: scripts/tag_ts_client.sh

root_dir="$(cd "$(dirname "$0")/.." && pwd)"
pkg="$root_dir/clients/typescript/package.json"
ver=$(jq -r .version "$pkg")
if [[ -z "$ver" || "$ver" == "null" ]]; then
  echo "error: could not read version from $pkg" >&2
  exit 1
fi
tag="ts-client-v$ver"
if git rev-parse "$tag" >/dev/null 2>&1; then
  echo "tag $tag already exists"
  exit 0
fi
git tag "$tag"
echo "created tag $tag"
echo "push with: git push origin $tag"

