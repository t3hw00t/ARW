#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)

HOSTS=()
if [[ $# -eq 0 ]]; then
  HOSTS=(localhost 127.0.0.1 ::1)
else
  for arg in "$@"; do
    [[ -n "$arg" ]] && HOSTS+=("$arg")
  done
fi

if [[ ${#HOSTS[@]} -eq 0 ]]; then
  echo "[dev-tls] no hostnames provided" >&2
  exit 1
fi

PRIMARY_HOST=${HOSTS[0]}
CONFIG_ROOT="$PROJECT_ROOT/configs/reverse_proxy"
CADDY_DIR="$CONFIG_ROOT/caddy"
CERT_PATH="$CADDY_DIR/$PRIMARY_HOST.crt"
KEY_PATH="$CADDY_DIR/$PRIMARY_HOST.key"
CADDYFILE_PATH="$CADDY_DIR/Caddyfile.$PRIMARY_HOST"
mkdir -p "$CADDY_DIR"

PORT_DEFAULT=${ARW_PORT:-8091}
BACKEND_PORT=${ARW_DEV_TLS_BACKEND_PORT:-$PORT_DEFAULT}

log() {
  local msg="$1"
  echo "[dev-tls] $msg"
}

use_mkcert=1
if [[ "${ARW_DEV_TLS_FORCE_OPENSSL:-0}" = "1" ]]; then
  use_mkcert=0
elif ! command -v mkcert >/dev/null 2>&1; then
  use_mkcert=0
fi

if [[ $use_mkcert -eq 1 ]]; then
  log "Using mkcert for ${HOSTS[*]}"
  if [[ "${ARW_DEV_TLS_SKIP_TRUST_INSTALL:-0}" != "1" ]]; then
    if mkcert -install >/dev/null 2>&1; then
      log "mkcert trust store installed"
    else
      log "mkcert trust install failed (continuing); rerun with ARW_DEV_TLS_SKIP_TRUST_INSTALL=1 to skip"
    fi
  fi
  mkcert -cert-file "$CERT_PATH" -key-file "$KEY_PATH" "${HOSTS[@]}"
else
  log "mkcert not available — generating self-signed certificate via openssl"
  TMP_CONF=$(mktemp -t arw-dev-tls.XXXXXX)
  trap 'rm -f "$TMP_CONF"' EXIT
  {
    echo "[req]"
    echo "distinguished_name = dn"
    echo "x509_extensions = v3_req"
    echo "prompt = no"
    echo "default_md = sha256"
    echo
    echo "[dn]"
    echo "CN = $PRIMARY_HOST"
    echo
    echo "[v3_req]"
    echo "subjectAltName = @alt_names"
    echo
    echo "[alt_names]"
    dns_index=1
    ip_index=1
    for host in "${HOSTS[@]}"; do
      if [[ $host =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]]; then
        printf 'IP.%d = %s\n' "$ip_index" "$host"
        ip_index=$((ip_index + 1))
      elif [[ $host =~ : ]]; then
        printf 'IP.%d = %s\n' "$ip_index" "$host"
        ip_index=$((ip_index + 1))
      else
        printf 'DNS.%d = %s\n' "$dns_index" "$host"
        dns_index=$((dns_index + 1))
      fi
    done
  } > "$TMP_CONF"

  openssl req -x509 -nodes \
    -days "${ARW_DEV_TLS_SELF_SIGNED_DAYS:-825}" \
    -newkey rsa:2048 \
    -keyout "$KEY_PATH" \
    -out "$CERT_PATH" \
    -config "$TMP_CONF" >/dev/null 2>&1
fi

chmod 600 "$KEY_PATH"

cat > "$CADDYFILE_PATH" <<CADDY
$PRIMARY_HOST {
  tls $CERT_PATH $KEY_PATH
  encode zstd gzip
  reverse_proxy 127.0.0.1:$BACKEND_PORT {
    header_up X-Forwarded-For {remote_host}
    header_up X-Forwarded-Proto {scheme}
    header_up X-Forwarded-Host {host}
  }
}
CADDY

log "TLS assets written to $CADDY_DIR"
log "Certificate: $CERT_PATH"
log "Private key: $KEY_PATH"
log "Caddyfile: $CADDYFILE_PATH"
log "Run 'caddy run --config $CADDYFILE_PATH' to start the proxy"
