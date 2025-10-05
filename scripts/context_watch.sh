#!/usr/bin/env bash
set -euo pipefail

OUTPUT_ROOT="docs/ops/trials/logs"
EXTRA_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-root)
      if [[ $# -lt 2 ]]; then
        echo "--output-root requires a value" >&2
        exit 1
      fi
      OUTPUT_ROOT="$2"
      shift 2
      ;;
    *)
      EXTRA_ARGS+=("$1")
      shift
      ;;
  esac
end

DATE_STAMP=$(date +%Y-%m-%d)
LOG_DIR="${OUTPUT_ROOT%/}/$DATE_STAMP"
LOG_PATH="$LOG_DIR/context.log"
mkdir -p "$LOG_DIR"

printf '[context-watch] writing to %s\n' "$LOG_PATH"
exec arw-cli context telemetry --watch --output "$LOG_PATH" "${EXTRA_ARGS[@]}"
