#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"

PORT_DEFAULT=8091
PORT=${ARW_PORT:-$PORT_DEFAULT}
PORT_SPECIFIED=0
if [[ -n "${ARW_PORT:-}" ]]; then PORT_SPECIFIED=1; fi
DOCS_URL=${ARW_DOCS_URL:-}
ADMIN_TOKEN=${ARW_ADMIN_TOKEN:-}
USE_DIST=0
NO_BUILD=0
OPEN_UI=0
DEBUG_PATH="/admin/debug"
WAIT_HEALTH=1
WAIT_HEALTH_TIMEOUT_SECS=${ARW_WAIT_HEALTH_TIMEOUT_SECS:-20}
INTERACTIVE=0

usage() {
  cat <<'EOF'
ARW debug helper

Usage: scripts/debug.sh [options]

Options
  -i, --interactive     Prompt for port/token and run
  --port N              HTTP port (default: 8091)
  --docs-url URL        Docs URL to advertise in UI
  --admin-token TOKEN   Admin token (recommended)
  --dist                Use latest dist/ bundle if present
  --no-build            Do not build if binary missing
  --open                Open admin debug UI after start
  --no-open             Do not open admin debug UI (default)
  --no-health           Do not wait for /healthz
  --health-timeout N    Health wait timeout seconds (default: 20)
  -h, --help            Show help

Examples
  scripts/debug.sh --port 8091 --open
  scripts/debug.sh --interactive
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -i|--interactive) INTERACTIVE=1; shift ;;
    --port) PORT="$2"; PORT_SPECIFIED=1; shift 2 ;;
    --docs-url) DOCS_URL="$2"; shift 2 ;;
    --admin-token) ADMIN_TOKEN="$2"; shift 2 ;;
    --dist) USE_DIST=1; shift ;;
    --no-build) NO_BUILD=1; shift ;;
    --open) OPEN_UI=1; shift ;;
    --no-open) OPEN_UI=0; shift ;;
    --no-health) WAIT_HEALTH=0; shift ;;
    --health-timeout) WAIT_HEALTH_TIMEOUT_SECS="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage; exit 2 ;;
  esac
done

prompt_interactive() {
  echo "Agent Hub (ARW) — Debug (interactive)"
  if [[ $PORT_SPECIFIED -eq 0 ]]; then
    PORT=$PORT_DEFAULT
  fi
  read -r -p "HTTP port [$PORT]: " ans; PORT=${ans:-$PORT}
  if [[ -n "$ans" ]]; then PORT_SPECIFIED=1; fi
  read -r -p "Docs URL (optional) [${DOCS_URL}]: " ans; DOCS_URL=${ans:-$DOCS_URL}
  if [[ -z "${ADMIN_TOKEN:-}" ]]; then
    read -r -p "Generate admin token? (Y/n): " yn
    if [[ "${yn,,}" != n* ]]; then
      ADMIN_TOKEN="$(head -c 24 /dev/urandom | base64 | tr -d '=+/\n' | head -c 32)"
      echo "[debug] Token: $ADMIN_TOKEN"
    fi
  fi
  read -r -p "Use dist/ if available? (y/N): " yn; [[ "${yn,,}" == y* ]] && USE_DIST=1 || USE_DIST=0
  read -r -p "Open admin debug UI after start? (y/N): " yn; [[ "${yn,,}" == y* ]] && OPEN_UI=1 || OPEN_UI=0
}

if [[ $INTERACTIVE -eq 1 ]]; then
  prompt_interactive
fi

if [[ $PORT_SPECIFIED -eq 0 ]]; then
  PORT=$PORT_DEFAULT
fi

args=( --port "$PORT" --debug )
[[ -n "${DOCS_URL:-}" ]] && args+=( --docs-url "$DOCS_URL" )
[[ -n "${ADMIN_TOKEN:-}" ]] && args+=( --admin-token "$ADMIN_TOKEN" )
[[ $USE_DIST -eq 1 ]] && args+=( --dist )
[[ $NO_BUILD -eq 1 ]] && args+=( --no-build )
if [[ $WAIT_HEALTH -eq 1 ]]; then
  args+=( --wait-health --wait-health-timeout-secs "$WAIT_HEALTH_TIMEOUT_SECS" )
fi

export ARW_DEBUG=1
export ARW_PORT="$PORT"
export ARW_DOCS_URL="${DOCS_URL:-}"
export ARW_ADMIN_TOKEN="${ADMIN_TOKEN:-}"

bash "$DIR/start.sh" "${args[@]}"

if [[ $OPEN_UI -eq 1 ]]; then
  base="http://127.0.0.1:$PORT"
  target="$base$DEBUG_PATH"
  if command -v xdg-open >/dev/null 2>&1; then xdg-open "$target" >/dev/null 2>&1 || true
  elif command -v open >/dev/null 2>&1; then open "$target" || true
  else "$DIR/open-url.sh" "$target" || true; fi
fi
