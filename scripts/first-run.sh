#!/usr/bin/env bash
set -euo pipefail

port="${ARW_PORT:-8091}"
with_launcher=0
force_new_token=0

usage() {
  cat <<'USAGE'
Usage: first-run.sh [options]
  --port N          Set ARW_PORT (default 8091)
  --launcher        Attempt to launch the desktop launcher alongside the service
  --new-token       Ignore saved token and generate a fresh admin token
  -h, --help        Show this help
Pass any remaining arguments after -- directly to arw-server.
USAGE
}

server_args=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)
      port="$2"
      shift 2
      ;;
    --launcher)
      with_launcher=1
      shift
      ;;
    --new-token)
      force_new_token=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      server_args+=("$@")
      break
      ;;
    *)
      echo "[first-run] Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -d "$script_dir/bin" ]]; then
  root="$script_dir"
  bin_dir="$root/bin"
elif [[ -d "$script_dir/../bin" ]]; then
  root="$(cd "$script_dir/.." && pwd)"
  bin_dir="$root/bin"
elif [[ -d "$script_dir/../target/release" ]]; then
  root="$(cd "$script_dir/.." && pwd)"
  bin_dir="$root/target/release"
else
  echo "[first-run] Unable to locate portable bundle outputs. Run from the extracted release directory or ensure target/release exists." >&2
  exit 1
fi

server_bin="$bin_dir/arw-server"
launcher_bin="$bin_dir/arw-launcher"

if [[ ! -x "$server_bin" ]]; then
  echo "[first-run] arw-server binary not found at $server_bin" >&2
  exit 1
fi

generate_token() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 32
    return 0
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

state_dir="$root/state"
token_file="$state_dir/admin-token.txt"
mkdir -p "$state_dir"

trim_token() {
  printf '%s' "$1" | tr -d '\r\n'
}

token="${ARW_ADMIN_TOKEN:-}"
if [[ -z "$token" || $force_new_token -eq 1 ]]; then
  if [[ -s "$token_file" && $force_new_token -eq 0 ]]; then
    token="$(trim_token "$(cat "$token_file")")"
    echo "[first-run] Reusing saved admin token from $token_file"
  else
    if ! token="$(generate_token)"; then
      echo "[first-run] Unable to generate an admin token. Install openssl or python3 (or set ARW_ADMIN_TOKEN manually)." >&2
      exit 1
    fi
    token="$(trim_token "$token")"
    printf '%s\n' "$token" >"$token_file"
    chmod 600 "$token_file" 2>/dev/null || true
    echo "[first-run] Generated new admin token and saved it to $token_file"
  fi
fi

export ARW_ADMIN_TOKEN="$token"
export ARW_PORT="$port"
export ARW_BIND="${ARW_BIND:-127.0.0.1}"

echo "[first-run] Admin token: $token"
echo "[first-run] Control Room: http://127.0.0.1:$ARW_PORT/admin/ui/control/"
echo "[first-run] Debug panels: http://127.0.0.1:$ARW_PORT/admin/debug"
echo "[first-run] Saved token file: $token_file"

if [[ $with_launcher -eq 1 ]]; then
  if [[ -x "$launcher_bin" ]]; then
    echo "[first-run] Starting service and launcher..."
    "$server_bin" "${server_args[@]}" &
    svc_pid=$!
    sleep 1
    "$launcher_bin" &
    launcher_pid=$!
    trap 'kill "$svc_pid" "$launcher_pid" 2>/dev/null || true' INT TERM
    wait "$svc_pid"
  else
    echo "[first-run] Launcher binary not available; falling back to headless mode." >&2
    exec "$server_bin" "${server_args[@]}"
  fi
else
  echo "[first-run] Starting service only on http://127.0.0.1:$ARW_PORT"
  exec "$server_bin" "${server_args[@]}"
fi
