#!/usr/bin/env bash
set -euo pipefail

if ! command -v arw-cli >/dev/null 2>&1; then
  echo "context-watch: arw-cli is not in PATH; build/install the CLI (e.g., cargo build -p arw-cli) before running." >&2
  exit 127
fi

OUTPUT_ROOT="${ARW_CONTEXT_WATCH_OUTPUT_ROOT:-docs/ops/trials/logs}"
BASE="${ARW_CONTEXT_WATCH_BASE:-http://127.0.0.1:8091}"
SESSION="${ARW_CONTEXT_WATCH_SESSION:-}"
EXTRA_ARGS=()
DATE_OVERRIDE=""

if [[ -n "$SESSION" && ! "$SESSION" =~ ^[A-Za-z0-9._-]+$ ]]; then
  echo "context-watch: ARW_CONTEXT_WATCH_SESSION must be alphanumeric (plus - _ .)" >&2
  exit 1
fi

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
    --base)
      if [[ $# -lt 2 ]]; then
        echo "--base requires a value" >&2
        exit 1
      fi
      BASE="$2"
      shift 2
      ;;
    --session)
      if [[ $# -lt 2 ]]; then
        echo "--session requires a value" >&2
        exit 1
      fi
      if [[ ! "$2" =~ ^[A-Za-z0-9._-]+$ ]]; then
        echo "--session must be alphanumeric (plus - _ .)" >&2
        exit 1
      fi
      SESSION="$2"
      shift 2
      ;;
    --date)
      if [[ $# -lt 2 ]]; then
        echo "--date requires a value" >&2
        exit 1
      fi
      if [[ ! "$2" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
        echo "--date must be in YYYY-MM-DD format" >&2
        exit 1
      fi
      DATE_OVERRIDE="$2"
      shift 2
      ;;
    --)
      shift
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
          --base)
            if [[ $# -lt 2 ]]; then
              echo "--base requires a value" >&2
              exit 1
            fi
            BASE="$2"
            shift 2
            ;;
          --session)
            if [[ $# -lt 2 ]]; then
              echo "--session requires a value" >&2
              exit 1
            fi
            if [[ ! "$2" =~ ^[A-Za-z0-9._-]+$ ]]; then
              echo "--session must be alphanumeric (plus - _ .)" >&2
              exit 1
            fi
            SESSION="$2"
            shift 2
            ;;
          --date)
            if [[ $# -lt 2 ]]; then
              echo "--date requires a value" >&2
              exit 1
            fi
            if [[ ! "$2" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]]; then
              echo "--date must be in YYYY-MM-DD format" >&2
              exit 1
            fi
            DATE_OVERRIDE="$2"
            shift 2
            ;;
          --)
            EXTRA_ARGS+=("$1")
            shift
            ;;
          *)
            EXTRA_ARGS+=("$1")
            shift
            ;;
        esac
      done
      break
      ;;
    *)
      EXTRA_ARGS+=("$1")
      shift
      ;;
  esac
done

if [[ -n "$DATE_OVERRIDE" ]]; then
  DATE_STAMP="$DATE_OVERRIDE"
else
  DATE_STAMP=$(date +%Y-%m-%d)
fi
LOG_DIR="${OUTPUT_ROOT%/}/$DATE_STAMP"
LOG_PATH="$LOG_DIR/context.log"
if [[ -n "$SESSION" ]]; then
  LOG_DIR="$LOG_DIR/$SESSION"
  LOG_PATH="$LOG_DIR/context.log"
fi
mkdir -p "$LOG_DIR"

printf '[context-watch] writing to %s\n' "$LOG_PATH"
exec arw-cli context telemetry --watch --base "$BASE" --output "$LOG_PATH" "${EXTRA_ARGS[@]}"
