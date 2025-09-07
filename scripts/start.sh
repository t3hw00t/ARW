#!/usr/bin/env bash
set -euo pipefail

port=8090
debug=0
docs_url=""
admin_token=""
timeout_secs=20
use_dist=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port) port="$2"; shift 2;;
    --debug) debug=1; shift;;
    --docs-url) docs_url="$2"; shift 2;;
    --admin-token) admin_token="$2"; shift 2;;
    --timeout-secs) timeout_secs="$2"; shift 2;;
    --dist) use_dist=1; shift;;
    -h|--help)
      echo "Usage: $0 [--port N] [--debug] [--docs-url URL] [--admin-token TOKEN] [--timeout-secs N] [--dist]"; exit 0;;
    *) echo "Unknown option: $1"; exit 1;;
  esac
done

export ARW_PORT="$port"
export ARW_HTTP_TIMEOUT_SECS="$timeout_secs"
[[ $debug -eq 1 ]] && export ARW_DEBUG=1 || true
[[ -n "$docs_url" ]] && export ARW_DOCS_URL="$docs_url" || true
[[ -n "$admin_token" ]] && export ARW_ADMIN_TOKEN="$admin_token" || true

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"

exe="arw-svc"; [[ "${OS:-}" == "Windows_NT" ]] && exe+=".exe"
if [[ $use_dist -eq 1 ]]; then
  base=$(ls -td "$ROOT"/dist/arw-* 2>/dev/null | head -n1 || true)
  svc="$base/bin/$exe"
else
  svc="$ROOT/target/release/$exe"
fi

if [[ ! -x "$svc" ]]; then
  echo "[start] Service binary not found ($svc). Building release..."
  (cd "$ROOT" && cargo build --release -p arw-svc)
  svc="$ROOT/target/release/$exe"
fi

echo "[start] Launching $svc on http://127.0.0.1:$ARW_PORT"
exec "$svc"
