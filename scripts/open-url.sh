#!/usr/bin/env bash
set -euo pipefail
url=${1:?Usage: open-url.sh <url>}

if command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$url" >/dev/null 2>&1 &
elif command -v open >/dev/null 2>&1; then
  open "$url" >/dev/null 2>&1 &
elif command -v powershell.exe >/dev/null 2>&1; then
  powershell.exe start "${url}"
else
  echo "Open: $url"
fi

