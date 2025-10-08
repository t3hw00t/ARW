#!/usr/bin/env bash
set -euo pipefail

port=""
port_set=0
debug=0
docs_url=""
admin_token=""
timeout_secs=20
use_dist=0
no_build=0
wait_health=0
wait_health_timeout_secs=30
pid_file="${ARW_PID_FILE:-}"
service_only=0
launcher_only=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)
      port="$2"
      port_set=1
      shift 2
      ;;
    --debug)
      debug=1
      shift
      ;;
    --docs-url)
      docs_url="$2"
      shift 2
      ;;
    --admin-token)
      admin_token="$2"
      shift 2
      ;;
    --timeout-secs)
      timeout_secs="$2"
      shift 2
      ;;
    --dist)
      use_dist=1
      shift
      ;;
    --no-build)
      no_build=1
      shift
      ;;
    --wait-health)
      wait_health=1
      shift
      ;;
    --wait-health-timeout-secs)
      wait_health_timeout_secs="$2"
      shift 2
      ;;
    --service-only)
      service_only=1
      shift
      ;;
    --launcher-only)
      launcher_only=1
      shift
      ;;
    -h|--help)
      cat <<USAGE
Usage: $0 [options]
  --port N                      Override HTTP port (default 8091)
  --debug                       Export ARW_DEBUG=1
  --docs-url URL                Export ARW_DOCS_URL
  --admin-token TOKEN           Export ARW_ADMIN_TOKEN
  --timeout-secs N              Export ARW_HTTP_TIMEOUT_SECS (default 20)
  --dist                        Use latest ./dist bundle instead of target/
  --no-build                    Do not auto-build missing binaries
  --wait-health                 Poll /healthz until ready
  --wait-health-timeout-secs N  Override health wait timeout (default 30)
  --service-only                Run the service binary only (no launcher)
  --launcher-only               Run launcher without starting service
USAGE
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

if [[ $service_only -eq 1 && $launcher_only -eq 1 ]]; then
  echo "[start] --service-only and --launcher-only cannot be combined" >&2
  exit 1
fi

if [[ $port_set -eq 0 ]]; then
  port=8091
fi

export ARW_PORT="$port"
export ARW_HTTP_TIMEOUT_SECS="$timeout_secs"
[[ $debug -eq 1 ]] && export ARW_DEBUG=1 || true
[[ -n "$docs_url" ]] && export ARW_DOCS_URL="$docs_url" || true
[[ -n "$admin_token" ]] && export ARW_ADMIN_TOKEN="$admin_token" || true
# Hardened defaults unless caller overrides
export ARW_EGRESS_PROXY_ENABLE="${ARW_EGRESS_PROXY_ENABLE:-1}"
export ARW_DNS_GUARD_ENABLE="${ARW_DNS_GUARD_ENABLE:-1}"

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"

exe="arw-server"
launcher_exe="arw-launcher"
if [[ "${OS:-}" == "Windows_NT" ]]; then
  exe+=".exe"
  launcher_exe+=".exe"
fi

if [[ $use_dist -eq 1 ]]; then
  base=$(ls -td "$ROOT"/dist/arw-* 2>/dev/null | head -n1 || true)
  if [[ -z "$base" ]]; then
    echo "[start] dist bundle not found; build a release package first or omit --dist" >&2
    exit 1
  fi
  svc="$base/bin/$exe"
  launcher="$base/bin/$launcher_exe"
else
  svc="$ROOT/target/release/$exe"
  launcher="$ROOT/target/release/$launcher_exe"
fi

ensure_service() {
  if [[ -x "$svc" ]]; then
    return 0
  fi
  if [[ $use_dist -eq 1 ]]; then
    echo "[start] Service binary missing in dist bundle ($svc). Re-run packaging or drop --dist." >&2
    exit 1
  fi
  if [[ $no_build -eq 1 ]]; then
    echo "[start] Service binary not found and --no-build specified. Build first or omit --no-build." >&2
    exit 1
  fi
  echo "[start] Service binary not found ($svc). Building release..."
  (cd "$ROOT" && cargo build --release -p arw-server)
}

ensure_launcher() {
  if [[ -x "$launcher" ]]; then
    return 0
  fi
  if [[ $launcher_only -eq 1 ]]; then
    if [[ $use_dist -eq 1 ]]; then
      echo "[start] Launcher binary missing in dist bundle ($launcher). Re-run packaging or drop --dist." >&2
      exit 1
    fi
    if [[ $no_build -eq 1 ]]; then
      echo "[start] Launcher binary not found and --no-build specified. Build first or omit --no-build." >&2
      exit 1
    fi
    echo "[start] Launcher binary not found ($launcher). Building release..."
    (cd "$ROOT" && cargo build --release -p arw-launcher)
  else
    if [[ $use_dist -eq 1 ]]; then
      echo "[start] Launcher binary missing in dist bundle ($launcher); falling back to service only." >&2
      launcher_only=0
      return 1
    fi
    if [[ $no_build -eq 1 ]]; then
      echo "[start] Launcher binary not found and --no-build specified; falling back to service only." >&2
      launcher_only=0
      return 1
    fi
    echo "[start] Launcher binary not found ($launcher). Attempting build..."
    (cd "$ROOT" && cargo build --release -p arw-launcher) || true
  fi
}

wait_for_health() {
  local base="http://127.0.0.1:$ARW_PORT"
  local deadline=$(( $(date +%s) + wait_health_timeout_secs ))
  local attempts=0
  while [[ $(date +%s) -lt $deadline ]]; do
    attempts=$((attempts+1))
    if command -v curl >/dev/null 2>&1; then
      if curl -fsS "$base/healthz" >/dev/null 2>&1; then
        echo "[start] Health OK after $attempts checks → $base/healthz"
        return 0
      fi
    elif command -v wget >/dev/null 2>&1; then
      if wget -qO- "$base/healthz" >/dev/null 2>&1; then
        echo "[start] Health OK after $attempts checks → $base/healthz"
        return 0
      fi
    fi
    sleep 0.5
  done
  echo "[start] Health not reachable within ${wait_health_timeout_secs}s → $base/healthz" >&2
  return 1
}

if [[ $launcher_only -eq 0 ]]; then
  ensure_service
fi
if [[ $launcher_only -eq 1 || $service_only -eq 0 ]]; then
  ensure_launcher || true
fi

if [[ $service_only -eq 1 ]]; then
  echo "[start] Starting service only on http://127.0.0.1:$ARW_PORT"
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
  if [[ $wait_health -eq 1 ]]; then
    wait_for_health || true
  fi
  wait
else
  if [[ "${ARW_NO_LAUNCHER:-0}" == "1" || "${ARW_NO_TRAY:-0}" == "1" ]]; then
    echo "[start] Headless requested via env (ARW_NO_LAUNCHER/ARW_NO_TRAY); forcing --service-only"
    exec "$0" --service-only "$@"
  fi
  if [[ ! -x "$launcher" ]]; then
    echo "[start] Launcher binary not found ($launcher); falling back to service only"
    exec "$0" --service-only "$@"
  fi
  if [[ $launcher_only -eq 0 ]]; then
    export ARW_AUTOSTART=1
  fi
  echo "[start] Launching $launcher (port $ARW_PORT)"
  "$launcher" &
  launcher_pid=$!
  if ! kill -0 "$launcher_pid" >/dev/null 2>&1; then
    wait "$launcher_pid"
    exit $?
  fi
  if [[ $wait_health -eq 1 && $launcher_only -eq 0 ]]; then
    wait_for_health || true
  fi
  wait "$launcher_pid"
fi
