#!/usr/bin/env bash
# shellcheck disable=SC2012
set -euo pipefail

port=8090
debug=0
docs_url=""
admin_token=""
timeout_secs=20
use_dist=0
no_build=0
wait_health=0
wait_health_timeout_secs=30
pid_file="${ARW_PID_FILE:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port) port="$2"; shift 2;;
    --debug) debug=1; shift;;
    --docs-url) docs_url="$2"; shift 2;;
    --admin-token) admin_token="$2"; shift 2;;
    --timeout-secs) timeout_secs="$2"; shift 2;;
    --dist) use_dist=1; shift;;
    --no-build) no_build=1; shift;;
    --wait-health) wait_health=1; shift;;
    --wait-health-timeout-secs) wait_health_timeout_secs="$2"; shift 2;;
    -h|--help)
      echo "Usage: $0 [--port N] [--debug] [--docs-url URL] [--admin-token TOKEN] [--timeout-secs N] [--dist] [--no-build] [--wait-health] [--wait-health-timeout-secs N]"; exit 0;;
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
launcher_exe="arw-launcher"; [[ "${OS:-}" == "Windows_NT" ]] && launcher_exe+=".exe"
if [[ $use_dist -eq 1 ]]; then
  base=$(ls -td "$ROOT"/dist/arw-* 2>/dev/null | head -n1 || true)
  svc="$base/bin/$exe"
  launcher="$base/bin/$launcher_exe"
else
  svc="$ROOT/target/release/$exe"
  launcher="$ROOT/target/release/$launcher_exe"
fi

if [[ ! -x "$svc" ]]; then
  if [[ $no_build -eq 1 ]]; then
    echo "[start] Service binary not found and --no-build specified. Build first or omit --no-build." >&2
    exit 1
  fi
  echo "[start] Service binary not found ($svc). Building release..."
  (cd "$ROOT" && cargo build --release -p arw-svc)
  svc="$ROOT/target/release/$exe"
fi

if [[ ! -x "$launcher" && $no_build -eq 0 ]]; then
  echo "[start] Launcher binary not found ($launcher). Attempting build..."
  (cd "$ROOT" && cargo build --release -p arw-launcher) || true
  launcher="$ROOT/target/release/$launcher_exe"
fi

echo "[start] Launching $svc on http://127.0.0.1:$ARW_PORT"
if [[ -n "${ARW_LOG_FILE:-}" ]]; then
  mkdir -p "$(dirname "$ARW_LOG_FILE")" || true
  echo "[start] Logging service output to $ARW_LOG_FILE"
  "$svc" >>"$ARW_LOG_FILE" 2>&1 &
else
  "$svc" &
fi
svc_pid=$!
if [[ -n "$pid_file" ]]; then
  mkdir -p "$(dirname "$pid_file")" || true
  echo "$svc_pid" > "$pid_file" || true
fi
wait_for_health() {
  local base="http://127.0.0.1:$ARW_PORT"
  local deadline=$(( $(date +%s) + wait_health_timeout_secs ))
  local attempts=0
  while [[ $(date +%s) -lt $deadline ]]; do
    attempts=$((attempts+1))
    if command -v curl >/dev/null 2>&1; then
      if curl -fsS "$base/healthz" >/dev/null 2>&1; then
        echo "[start] Health OK after $attempts checks → $base/healthz"; return 0
      fi
    elif command -v wget >/dev/null 2>&1; then
      if wget -qO- "$base/healthz" >/dev/null 2>&1; then
        echo "[start] Health OK after $attempts checks → $base/healthz"; return 0
      fi
    fi
    sleep 0.5
  done
  echo "[start] Health not reachable within ${wait_health_timeout_secs}s → $base/healthz" >&2
  return 1
}

if [[ $wait_health -eq 1 ]]; then
  wait_for_health || true
fi

if [[ "${ARW_NO_TRAY:-0}" == "1" ]]; then
  echo "[start] ARW_NO_TRAY=1; skipping launcher; service running in background"
  wait
elif [[ -x "$launcher" ]]; then
  echo "[start] Launching launcher $launcher"
  exec "$launcher"
else
  echo "[start] Launcher binary not found ($launcher); service running in background"
  wait
fi
