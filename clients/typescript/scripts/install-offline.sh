#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

CACHE_DIR="${CACHE_DIR:-.npm-cache}"
CACHE_TAR="${CACHE_TAR:-npm-cache.tgz}"
REFRESH="${REFRESH:-0}"

if [[ "${1:-}" == "--refresh" ]]; then
  REFRESH=1
  shift
fi

if [[ "$REFRESH" == "1" ]]; then
  rm -rf "$CACHE_DIR" "$CACHE_TAR"
fi

if [[ -f "$CACHE_TAR" ]]; then
  tar -xzf "$CACHE_TAR"
fi

if [[ -d "$CACHE_DIR" ]]; then
  npm ci --offline --prefer-offline --cache "$CACHE_DIR"
else
  npm ci --prefer-offline --cache "$CACHE_DIR"
fi

tar -czf "$CACHE_TAR" "$CACHE_DIR"
