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
launcher_build_failed=0
settings_port=""
setting_autostart=""
setting_notify=""
setting_base_override=""
launcher_settings_loaded=0

launcher_config_dir() {
  local base=""
  local append_suffix=1
  case "${OSTYPE:-}" in
    darwin*)
      if [[ -n "${ARW_CONFIG_HOME:-}" ]]; then
        base="$ARW_CONFIG_HOME"
        append_suffix=0
      elif [[ -n "${XDG_CONFIG_HOME:-}" ]]; then
        base="$XDG_CONFIG_HOME"
      else
        base="$HOME/Library/Application Support"
      fi
      ;;
    *)
      if [[ -n "${ARW_CONFIG_HOME:-}" ]]; then
        base="$ARW_CONFIG_HOME"
        append_suffix=0
      elif [[ -n "${XDG_CONFIG_HOME:-}" ]]; then
        base="$XDG_CONFIG_HOME"
      else
        base="$HOME/.config"
      fi
      ;;
  esac
  # shellcheck disable=SC2001
  base="$(printf '%s' "${base%/}" | sed 's:/*$::')"
  if [[ $append_suffix -eq 1 ]]; then
    printf '%s/arw' "$base"
  else
    printf '%s' "$base"
  fi
}

persist_launcher_prefs() {
  local token="$1"
  local port_value="$2"
  [[ -z "$token" && -z "$port_value" ]] && return 0
  local dir
  dir="$(launcher_config_dir)"
  [[ -n "$dir" ]] || return 0
  mkdir -p "$dir" || return 0
  local prefs="$dir/prefs-launcher.json"
  if command -v python3 >/dev/null 2>&1; then
    ARW_LAUNCHER_PREFS_PATH="$prefs" \
    ARW_LAUNCHER_PREFS_TOKEN="$token" \
    ARW_LAUNCHER_PREFS_PORT="$port_value" \
    python3 <<'PY' || return 0
import json, os, pathlib

path = pathlib.Path(os.environ["ARW_LAUNCHER_PREFS_PATH"])
token = os.environ.get("ARW_LAUNCHER_PREFS_TOKEN") or ""
port = os.environ.get("ARW_LAUNCHER_PREFS_PORT") or ""

data = {}
if path.exists():
    try:
        data = json.loads(path.read_text())
    except Exception:
        data = {}
if not isinstance(data, dict):
    data = {}

if token:
    data["adminToken"] = token
if port:
    try:
        data["port"] = int(port)
    except Exception:
        data["port"] = port

path.write_text(json.dumps(data, indent=2) + "\n")
PY
    return 0
  fi

  if command -v jq >/dev/null 2>&1; then
    local tmp tmp_out
    tmp="$(mktemp "${TMPDIR:-/tmp}/arw-launcher-prefs.XXXXXX")"
    tmp_out="${tmp}.out"
    if [[ -s "$prefs" ]]; then
      if ! jq 'if type=="object" then . else {} end' "$prefs" >"$tmp" 2>/dev/null; then
        printf '{}' >"$tmp"
      fi
    else
      printf '{}' >"$tmp"
    fi
    local filter='.'
    local -a jq_args
    jq_args=(--sort-keys)
    if [[ -n "$token" ]]; then
      jq_args+=(--arg token "$token")
      filter="$filter | .adminToken = \$token"
    fi
    if [[ -n "$port_value" ]]; then
      jq_args+=(--arg port "$port_value")
      filter="$filter | .port = (if (\$port | tonumber? // null) != null then (\$port | tonumber) else \$port end)"
    fi
    if jq "${jq_args[@]}" "$filter" "$tmp" >"$tmp_out" 2>/dev/null; then
      mv "$tmp_out" "$prefs"
    else
      echo "[start] Warning: unable to persist launcher prefs via jq fallback" >&2
      rm -f "$tmp_out"
    fi
    rm -f "$tmp"
    return 0
  fi

  echo "[start] Warning: python3 and jq unavailable; launcher prefs not updated. Save the token inside Control Room  Connection & alerts instead." >&2
}

load_launcher_settings() {
  settings_port=""
  setting_autostart=""
  setting_notify=""
  setting_base_override=""
  local dir
  dir="$(launcher_config_dir)"
  [[ -n "$dir" ]] || return 1
  local prefs="$dir/prefs-launcher.json"
  [[ -s "$prefs" ]] || return 1
  local output=""
  if command -v python3 >/dev/null 2>&1; then
    output="$(python3 - "$prefs" <<'PY' 2>/dev/null
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
try:
    data = json.loads(path.read_text())
except Exception:
    sys.exit(0)
if not isinstance(data, dict):
    sys.exit(0)
port = data.get('port')
if isinstance(port, int):
    print(f"port={port}")
else:
    try:
        print(f"port={int(port)}")
    except Exception:
        pass
auto = data.get('autostart')
if isinstance(auto, bool):
    print(f"autostart={1 if auto else 0}")
notify = data.get('notifyOnStatus')
if isinstance(notify, bool):
    print(f"notify={1 if notify else 0}")
base = data.get('baseOverride')
if isinstance(base, str):
    base = base.strip()
    if base:
        print(f"base={base}")
PY
)"
  elif command -v jq >/dev/null 2>&1; then
    output="$(jq -r '
      def emit($k; $v): if $v == null then empty else "\($k)=\($v)" end;
      . as $root |
      (emit("port"; (if ($root.port|type) == "number" then ($root.port|floor) else null end))),
      (emit("autostart"; (if ($root.autostart|type) == "boolean" then (if $root.autostart then 1 else 0 end) else null end))),
      (emit("notify"; (if ($root.notifyOnStatus|type) == "boolean" then (if $root.notifyOnStatus then 1 else 0 end) else null end))),
      (emit("base"; (if ($root.baseOverride|type) == "string" then ($root.baseOverride|gsub("^\\s+|\\s+$";"")) else null end)))
    ' "$prefs" 2>/dev/null)"
  else
    return 1
  fi
  [[ -n "$output" ]] || return 1
  while IFS='=' read -r key value; do
    case "$key" in
      port)
        [[ -n "$value" ]] && settings_port="$value"
        ;;
      autostart)
        setting_autostart="$value"
        ;;
      notify)
        setting_notify="$value"
        ;;
      base)
        setting_base_override="$value"
        ;;
    esac
  done <<<"$output"
  return 0
}

trim_token() {
  printf '%s' "$1" | tr -d '\r\n'
}

generate_admin_token() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 32 && return 0
  fi
  if command -v python3 >/dev/null 2>&1; then
    python3 - <<'PY'
import secrets
print(secrets.token_hex(32))
PY
    return 0
  fi
  if command -v python >/dev/null 2>&1; then
    python - <<'PY'
import secrets
print(secrets.token_hex(32))
PY
    return 0
  fi
  if command -v uuidgen >/dev/null 2>&1; then
    uuidgen | tr -d '[:space:]-' | cut -c1-32
    return 0
  fi
  return 1
}

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
    --launcher-debug)
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
  --launcher-debug              Alias for --debug
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

if load_launcher_settings; then
  launcher_settings_loaded=1
fi

if [[ $service_only -eq 1 && $launcher_only -eq 1 ]]; then
  echo "[start] --service-only and --launcher-only cannot be combined" >&2
  exit 1
fi

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"

ensure_admin_token() {
  local state_dir="${ARW_STATE_DIR:-$ROOT/state}"
  local token_file="$state_dir/admin-token.txt"
  mkdir -p "$state_dir" || return 1
  if [[ -s "$token_file" ]]; then
    local token
    token="$(trim_token "$(cat "$token_file")")"
    if [[ -n "$token" ]]; then
      export ARW_ADMIN_TOKEN="$token"
      admin_token="$token"
      echo "[start] Reusing admin token from $token_file"
      return 0
    fi
  fi
  local token=""
  if ! token="$(generate_admin_token)"; then
    echo "[start] Unable to generate an admin token automatically. Set ARW_ADMIN_TOKEN or use --admin-token." >&2
    return 1
  fi
  token="$(trim_token "$token")"
  printf '%s\n' "$token" >"$token_file"
  chmod 600 "$token_file" 2>/dev/null || true
  export ARW_ADMIN_TOKEN="$token"
  admin_token="$token"
  echo "[start] Generated admin token and saved to $token_file"
  return 0
}

if [[ $port_set -eq 0 ]]; then
  if [[ -n "$settings_port" && "$settings_port" =~ ^[0-9]+$ && $settings_port -ge 1 && $settings_port -le 65535 ]]; then
    port="$settings_port"
  else
    port=8091
  fi
fi

export ARW_PORT="$port"
export ARW_HTTP_TIMEOUT_SECS="$timeout_secs"
if [[ $debug -eq 1 ]]; then
  export ARW_DEBUG=1
fi
if [[ -n "$docs_url" ]]; then
  export ARW_DOCS_URL="$docs_url"
fi
if [[ -n "$admin_token" ]]; then
  export ARW_ADMIN_TOKEN="$admin_token"
fi
# Hardened defaults unless caller overrides
export ARW_EGRESS_PROXY_ENABLE="${ARW_EGRESS_PROXY_ENABLE:-1}"
export ARW_DNS_GUARD_ENABLE="${ARW_DNS_GUARD_ENABLE:-1}"

if [[ $launcher_only -eq 0 ]]; then
  if [[ -z "${ARW_ADMIN_TOKEN:-}" ]]; then
    ensure_admin_token || exit 1
  else
    # Ensure the saved token exists for future runs
    saved_state_dir="${ARW_STATE_DIR:-$ROOT/state}"
    saved_token_file="$saved_state_dir/admin-token.txt"
    if [[ ! -s "$saved_token_file" ]]; then
      mkdir -p "$saved_state_dir" || true
      printf '%s\n' "$(trim_token "${ARW_ADMIN_TOKEN}")" >"$saved_token_file" 2>/dev/null || true
      chmod 600 "$saved_token_file" 2>/dev/null || true
      echo "[start] Saved admin token to $saved_token_file"
    fi
  fi
fi

persist_token="${ARW_ADMIN_TOKEN:-}"
persist_port=""
if [[ $port_set -eq 1 ]]; then
  persist_port="$port"
fi
if [[ -n "$persist_token" || -n "$persist_port" ]]; then
  persist_launcher_prefs "$persist_token" "$persist_port"
fi

exe="arw-server"
launcher_exe="arw-launcher"
if [[ "${OS:-}" == "Windows_NT" ]]; then
  exe+=".exe"
  launcher_exe+=".exe"
fi

launcher_deps_checked=0

check_launcher_deps() {
  if [[ $launcher_deps_checked -eq 1 ]]; then
    return 0
  fi
  launcher_deps_checked=1
  if [[ "${OSTYPE:-}" != linux* ]]; then
    return 0
  fi
  if ! command -v pkg-config >/dev/null 2>&1; then
    echo "[start] Warning: pkg-config not found; unable to verify WebKitGTK 4.1 + libsoup3. If the launcher build fails, run scripts/install-tauri-deps.sh or see docs/guide/compatibility.md." >&2
    return 0
  fi
  if pkg-config --exists webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0 >/dev/null 2>&1; then
    return 0
  fi
  echo "[start] Warning: WebKitGTK 4.1 + libsoup3 development packages were not detected. The launcher build may fail. Run scripts/install-tauri-deps.sh or review docs/guide/compatibility.md." >&2
}

# shellcheck disable=SC2012
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
    check_launcher_deps
    if (cd "$ROOT" && cargo build --release -p arw-launcher); then
      :
    else
      echo "[start] Launcher build failed. The desktop UI requires WebKitGTK 4.1 + libsoup3 on Linux (scripts/install-tauri-deps.sh)." >&2
      exit 1
    fi
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
    check_launcher_deps
    if (cd "$ROOT" && cargo build --release -p arw-launcher); then
      :
    else
      launcher_build_failed=1
      echo "[start] Launcher build failed; continuing with service only."
      echo "[start] Hint: install WebKitGTK 4.1 + libsoup3 (scripts/install-tauri-deps.sh) or see docs/guide/compatibility.md."
    fi
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

if [[ $launcher_settings_loaded -eq 1 ]]; then
  summary_parts=("port $port")
  if [[ "$setting_autostart" == "1" ]]; then
    summary_parts+=("autostart on")
  elif [[ "$setting_autostart" == "0" ]]; then
    summary_parts+=("autostart off")
  fi
  if [[ "$setting_notify" == "1" ]]; then
    summary_parts+=("notifications on")
  elif [[ "$setting_notify" == "0" ]]; then
    summary_parts+=("notifications off")
  fi
  summary_text="${summary_parts[0]}"
  for item in "${summary_parts[@]:1}"; do
    summary_text+=", $item"
  done
  echo "[start] Launcher settings → $summary_text"
  if [[ -n "$setting_base_override" ]]; then
    echo "[start] Default base override → $setting_base_override"
  fi
  echo "[start] Adjust via Control Room → Launcher Settings."
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
    if [[ $launcher_build_failed -eq 1 ]]; then
      echo "[start] Hint: install WebKitGTK 4.1 + libsoup3 (scripts/install-tauri-deps.sh) or review docs/guide/compatibility.md before rebuilding the launcher."
    fi
    exec "$0" --service-only "$@"
  fi
  if [[ $launcher_only -eq 0 ]]; then
    if [[ ${ARW_AUTOSTART+x} ]]; then
      :
    elif [[ $launcher_settings_loaded -eq 1 ]]; then
      if [[ "$setting_autostart" == "1" ]]; then
        export ARW_AUTOSTART=1
      elif [[ "$setting_autostart" == "0" ]]; then
        unset ARW_AUTOSTART || true
      else
        export ARW_AUTOSTART=1
      fi
    else
      export ARW_AUTOSTART=1
    fi
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
