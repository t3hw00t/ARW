#!/usr/bin/env bash
set -euo pipefail

# ARW — Quick debug runner (standard + interactive)
# Thin wrapper over scripts/start.sh with ARW_DEBUG=1 and convenience flags.

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"

PORT=${ARW_PORT:-8090}
DOCS_URL=${ARW_DOCS_URL:-}
ADMIN_TOKEN=${ARW_ADMIN_TOKEN:-}
USE_DIST=0
NO_BUILD=0
OPEN_UI=1
WAIT_HEALTH=1
WAIT_HEALTH_TIMEOUT_SECS=${ARW_WAIT_HEALTH_TIMEOUT_SECS:-20}
INTERACTIVE=0

usage() {
  cat <<'EOF'
ARW debug helper

Usage: scripts/debug.sh [options]

Options
  -i, --interactive     Prompt for port/token and run
  --port N              HTTP port (default: 8090)
  --docs-url URL        Docs URL to advertise in UI
  --admin-token TOKEN   Admin token (recommended)
  --dist                Use latest dist/ bundle if present
  --no-build            Do not build if binary missing
  --no-open             Do not open /debug in browser
  --no-health           Do not wait for /healthz
  --health-timeout N    Health wait timeout seconds (default: 20)
  -h, --help            Show help

Examples
  scripts/debug.sh --port 8091 --no-open
  scripts/debug.sh --interactive
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -i|--interactive) INTERACTIVE=1; shift ;;
    --port) PORT="$2"; shift 2 ;;
    --docs-url) DOCS_URL="$2"; shift 2 ;;
    --admin-token) ADMIN_TOKEN="$2"; shift 2 ;;
    --dist) USE_DIST=1; shift ;;
    --no-build) NO_BUILD=1; shift ;;
    --no-open) OPEN_UI=0; shift ;;
    --no-health) WAIT_HEALTH=0; shift ;;
    --health-timeout) WAIT_HEALTH_TIMEOUT_SECS="$2"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown option: $1" >&2; usage; exit 2 ;;
  esac
done

prompt_interactive() {
  echo "ARW — Debug (interactive)"
  read -r -p "HTTP port [$PORT]: " ans; PORT=${ans:-$PORT}
  read -r -p "Docs URL (optional) [${DOCS_URL}]: " ans; DOCS_URL=${ans:-$DOCS_URL}
  if [[ -z "${ADMIN_TOKEN:-}" ]]; then
    read -r -p "Generate admin token? (Y/n): " yn; if [[ "${yn,,}" != n* ]]; then
      ADMIN_TOKEN="$(head -c 24 /dev/urandom | base64 | tr -d '=+/\n' | head -c 32)"
      echo "[debug] Token: $ADMIN_TOKEN"
    fi
  fi
  read -r -p "Use dist/ if available? (y/N): " yn; [[ "${yn,,}" == y* ]] && USE_DIST=1 || USE_DIST=0
  read -r -p "Open /debug after start? (Y/n): " yn; [[ "${yn,,}" == n* ]] && OPEN_UI=0 || OPEN_UI=1
}

if [[ $INTERACTIVE -eq 1 ]]; then
  prompt_interactive
fi

args=( --port "$PORT" --debug )
[[ -n "${DOCS_URL:-}" ]] && args+=( --docs-url "$DOCS_URL" )
[[ -n "${ADMIN_TOKEN:-}" ]] && args+=( --admin-token "$ADMIN_TOKEN" )
[[ $USE_DIST -eq 1 ]] && args+=( --dist )
[[ $NO_BUILD -eq 1 ]] && args+=( --no-build )
if [[ $WAIT_HEALTH -eq 1 ]]; then args+=( --wait-health --wait-health-timeout-secs "$WAIT_HEALTH_TIMEOUT_SECS" ); fi

export ARW_DEBUG=1
export ARW_PORT="$PORT"
export ARW_DOCS_URL="${DOCS_URL:-}"
export ARW_ADMIN_TOKEN="${ADMIN_TOKEN:-}"

bash "$DIR/start.sh" "${args[@]}"

if [[ $OPEN_UI -eq 1 ]]; then
  base="http://127.0.0.1:$PORT"
  if command -v xdg-open >/dev/null 2>&1; then xdg-open "$base/debug" >/dev/null 2>&1 || true
  elif command -v open >/dev/null 2>&1; then open "$base/debug" || true
  else "$DIR/open-url.sh" "$base/debug" || true; fi
fi

