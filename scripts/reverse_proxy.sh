#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
CONFIG_ROOT="$PROJECT_ROOT/configs/reverse_proxy"
RUN_ROOT="$PROJECT_ROOT/.arw/reverse_proxy"
LOG_ROOT="$PROJECT_ROOT/.arw/logs"
mkdir -p "$CONFIG_ROOT" "$RUN_ROOT" "$LOG_ROOT"

usage() {
  cat <<'USAGE'
Usage: reverse_proxy.sh <caddy|nginx> <generate|start|stop> [options]

Caddy options:
  generate [--host HOST] [--backend ADDR] [--email EMAIL] [--tls-module MODULE]
           [--config-name NAME]
    --host HOST           Hostname to serve (default: localhost)
    --backend ADDR        Backend address (default: 127.0.0.1:${ARW_PORT:-8091})
    --email EMAIL         ACME email for Let's Encrypt HTTP-01 verification
    --tls-module MODULE   Use Caddy DNS module (e.g., cloudflare) instead of HTTP-01
    --config-name NAME    Override generated config filename slug

  start [--host HOST] [--config-name NAME]
  stop [--host HOST] [--config-name NAME]

Nginx options:
  generate --host HOST --cert CERT --key KEY [--backend ADDR] [--config-name NAME]
    --host HOST           Hostname / server_name (required)
    --cert CERT           Path to TLS certificate (required)
    --key KEY             Path to TLS private key (required)
    --backend ADDR        Backend address (default: 127.0.0.1:${ARW_PORT:-8091})
    --config-name NAME    Override generated config filename slug

  start [--host HOST] [--config-name NAME]
  stop [--host HOST] [--config-name NAME]

Examples:
  scripts/reverse_proxy.sh caddy generate --host arw.example.com --email ops@example.com
  scripts/reverse_proxy.sh caddy start --host arw.example.com
  scripts/reverse_proxy.sh nginx generate --host arw.example.com --cert /path/fullchain.pem --key /path/privkey.pem
  scripts/reverse_proxy.sh nginx start --host arw.example.com
USAGE
}

error() {
  echo "[reverse-proxy] $1" >&2
  exit 1
}

need_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    error "Required command '$1' not found in PATH"
  fi
}

slugify() {
  local value="$1"
  value=$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')
  printf '%s' "$value" | sed 's/[^a-z0-9._-]/_/g'
}

caddy_config_path() {
  local slug="$1"
  printf '%s/caddy/Caddyfile.%s' "$CONFIG_ROOT" "$slug"
}

caddy_pid_path() {
  local slug="$1"
  printf '%s/caddy.%s.pid' "$RUN_ROOT" "$slug"
}

ensure_parent() {
  local path="$1"
  mkdir -p "$(dirname "$path")"
}

caddy_generate() {
  local host="localhost"
  local backend="127.0.0.1:${ARW_PORT:-8091}"
  local email=""
  local tls_module=""
  local config_name=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --host)
        host="$2"; shift 2 ;;
      --backend)
        backend="$2"; shift 2 ;;
      --email)
        email="$2"; shift 2 ;;
      --tls-module)
        tls_module="$2"; shift 2 ;;
      --config-name)
        config_name="$2"; shift 2 ;;
      --help|-h)
        usage; exit 0 ;;
      *)
        error "Unknown option for caddy generate: $1" ;;
    esac
  done

  local slug
  if [[ -n "$config_name" ]]; then
    slug=$(slugify "$config_name")
  else
    slug=$(slugify "$host")
  fi
  local config_path; config_path=$(caddy_config_path "$slug")
  ensure_parent "$config_path"

  local tls_block
  if [[ -n "$tls_module" ]]; then
    tls_block=$'  tls {\n    dns '
    tls_block+=$tls_module
    tls_block+=$'\n  }'
  elif [[ -n "$email" ]]; then
    tls_block="  tls $email"
  else
    tls_block="  tls internal  # Self-signed for local development"
  fi

  cat >"$config_path" <<EOF
$host {
  encode zstd gzip
$tls_block
  reverse_proxy $backend {
    header_up X-Forwarded-For {remote_host}
    header_up X-Forwarded-Proto {scheme}
    header_up X-Forwarded-Host {host}
  }

  @sse {
    header Connection *keep-alive*
  }
  handle @sse {
    reverse_proxy $backend
  }
}
EOF

  echo "[reverse-proxy] Caddy config written to $config_path"
  if [[ -n "$email" ]]; then
    echo "[reverse-proxy] Ensure ports 80/443 are reachable for HTTP-01 challenges"
  elif [[ -n "$tls_module" ]]; then
    echo "[reverse-proxy] Remember to export provider credentials required by the DNS module"
  else
    echo "[reverse-proxy] Using Caddy internal CA (self-signed). Browsers must trust Caddy's root CA."
  fi
}

caddy_start() {
  need_command caddy
  local host="localhost"
  local config_name=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --host)
        host="$2"; shift 2 ;;
      --config-name)
        config_name="$2"; shift 2 ;;
      --help|-h)
        usage; exit 0 ;;
      *)
        error "Unknown option for caddy start: $1" ;;
    esac
  done
  local slug
  if [[ -n "$config_name" ]]; then
    slug=$(slugify "$config_name")
  else
    slug=$(slugify "$host")
  fi
  local config_path; config_path=$(caddy_config_path "$slug")
  [[ -f "$config_path" ]] || error "Caddy config not found: $config_path (generate it first)"
  local pid_path; pid_path=$(caddy_pid_path "$slug")
  ensure_parent "$pid_path"

  if [[ -f "$pid_path" ]] && kill -0 "$(cat "$pid_path")" 2>/dev/null; then
    echo "[reverse-proxy] Caddy already running (pid $(cat "$pid_path"))"
    return
  fi

  echo "[reverse-proxy] Starting Caddy with $config_path"
  caddy start --config "$config_path" --pidfile "$pid_path"
}

caddy_stop() {
  need_command caddy
  local host="localhost"
  local config_name=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --host)
        host="$2"; shift 2 ;;
      --config-name)
        config_name="$2"; shift 2 ;;
      --help|-h)
        usage; exit 0 ;;
      *)
        error "Unknown option for caddy stop: $1" ;;
    esac
  done
  local slug
  if [[ -n "$config_name" ]]; then
    slug=$(slugify "$config_name")
  else
    slug=$(slugify "$host")
  fi
  local pid_path; pid_path=$(caddy_pid_path "$slug")
  if [[ -f "$pid_path" ]]; then
    local pid; pid=$(cat "$pid_path")
    if kill -0 "$pid" 2>/dev/null; then
      echo "[reverse-proxy] Stopping Caddy (pid $pid)"
      caddy stop --pidfile "$pid_path" >/dev/null 2>&1 || true
      sleep 0.5
      if kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
      fi
    fi
    rm -f "$pid_path"
  else
    echo "[reverse-proxy] No Caddy pid file for slug '$slug'"
  fi
}

nginx_config_dir() {
  local slug="$1"
  printf '%s/nginx/%s' "$CONFIG_ROOT" "$slug"
}

nginx_config_path() {
  local slug="$1"
  printf '%s/arw.conf' "$(nginx_config_dir "$slug")"
}

nginx_pid_path() {
  local slug="$1"
  printf '%s/nginx.%s.pid' "$RUN_ROOT" "$slug"
}

nginx_generate() {
  local host=""
  local backend="127.0.0.1:${ARW_PORT:-8091}"
  local cert=""
  local key=""
  local config_name=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --host)
        host="$2"; shift 2 ;;
      --backend)
        backend="$2"; shift 2 ;;
      --cert)
        cert="$2"; shift 2 ;;
      --key)
        key="$2"; shift 2 ;;
      --config-name)
        config_name="$2"; shift 2 ;;
      --help|-h)
        usage; exit 0 ;;
      *)
        error "Unknown option for nginx generate: $1" ;;
    esac
  done

  [[ -n "$host" ]] || error "--host is required for nginx generate"
  [[ -n "$cert" ]] || error "--cert is required for nginx generate"
  [[ -n "$key" ]] || error "--key is required for nginx generate"

  local slug
  if [[ -n "$config_name" ]]; then
    slug=$(slugify "$config_name")
  else
    slug=$(slugify "$host")
  fi
  local dir; dir=$(nginx_config_dir "$slug")
  mkdir -p "$dir/logs"
  local config_path; config_path=$(nginx_config_path "$slug")

  cat >"$config_path" <<EOF
events {}

http {
  log_format arw_combined '
    "\$remote_addr" - "\$remote_user" [\$time_local]
    "\$request" \$status \$body_bytes_sent
    "\$http_referer" "\$http_user_agent"';

  server {
    listen 443 ssl http2;
    server_name $host;

    ssl_certificate      $cert;
    ssl_certificate_key  $key;

    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers HIGH:!aNULL:!MD5;

    client_max_body_size 32m;

    location / {
      proxy_http_version 1.1;
      proxy_set_header Host \$host;
      proxy_set_header X-Forwarded-Proto \$scheme;
      proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
      proxy_set_header X-Real-IP \$remote_addr;
      proxy_buffering off;
      proxy_pass http://$backend;
    }

    access_log logs/access.log arw_combined;
    error_log  logs/error.log warn;
  }

  server {
    listen 80;
    server_name $host;
    return 301 https://\$host\$request_uri;
  }
}
EOF

  echo "[reverse-proxy] Nginx config written to $config_path"
  echo "[reverse-proxy] Logs stored under $dir/logs"
}

nginx_start() {
  need_command nginx
  local host="localhost"
  local config_name=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --host)
        host="$2"; shift 2 ;;
      --config-name)
        config_name="$2"; shift 2 ;;
      --help|-h)
        usage; exit 0 ;;
      *)
        error "Unknown option for nginx start: $1" ;;
    esac
  done
  local slug
  if [[ -n "$config_name" ]]; then
    slug=$(slugify "$config_name")
  else
    slug=$(slugify "$host")
  fi
  local dir; dir=$(nginx_config_dir "$slug")
  local config_path; config_path=$(nginx_config_path "$slug")
  [[ -f "$config_path" ]] || error "Nginx config not found: $config_path (generate it first)"
  local pid_path; pid_path=$(nginx_pid_path "$slug")
  ensure_parent "$pid_path"

  if [[ -f "$pid_path" ]] && kill -0 "$(cat "$pid_path")" 2>/dev/null; then
    echo "[reverse-proxy] Nginx already running (pid $(cat "$pid_path"))"
    return
  fi

  echo "[reverse-proxy] Starting Nginx with $config_path"
  nginx -p "$dir" -c "$(basename "$config_path")" -g "pid $pid_path;" >/dev/null 2>&1 || {
    echo "[reverse-proxy] Nginx failed to start; attempting to show error log" >&2
    if [[ -f "$dir/logs/error.log" ]]; then
      tail -n 20 "$dir/logs/error.log" >&2 || true
    fi
    exit 1
  }
}

nginx_stop() {
  local host="localhost"
  local config_name=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --host)
        host="$2"; shift 2 ;;
      --config-name)
        config_name="$2"; shift 2 ;;
      --help|-h)
        usage; exit 0 ;;
      *)
        error "Unknown option for nginx stop: $1" ;;
    esac
  done
  local slug
  if [[ -n "$config_name" ]]; then
    slug=$(slugify "$config_name")
  else
    slug=$(slugify "$host")
  fi
  local pid_path; pid_path=$(nginx_pid_path "$slug")
  if [[ -f "$pid_path" ]]; then
    local pid; pid=$(cat "$pid_path")
    if kill -0 "$pid" 2>/dev/null; then
      echo "[reverse-proxy] Stopping Nginx (pid $pid)"
      kill "$pid" 2>/dev/null || true
      wait "$pid" 2>/dev/null || true
    fi
    rm -f "$pid_path"
  else
    echo "[reverse-proxy] No Nginx pid file for slug '$slug'"
  fi
}

main() {
  if [[ $# -lt 2 ]]; then
    usage
    exit 1
  fi
  local provider="$1"; shift
  local action="$1"; shift

  case "$provider:$action" in
    caddy:generate)
      caddy_generate "$@" ;;
    caddy:start)
      caddy_start "$@" ;;
    caddy:stop)
      caddy_stop "$@" ;;
    nginx:generate)
      nginx_generate "$@" ;;
    nginx:start)
      nginx_start "$@" ;;
    nginx:stop)
      nginx_stop "$@" ;;
    *)
      usage
      exit 1 ;;
  esac
}

main "$@"
