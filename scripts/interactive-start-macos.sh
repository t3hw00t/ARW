#!/usr/bin/env bash
# shellcheck disable=SC1091
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"
# shellcheck source=lib/interactive_common.sh
. "$DIR/lib/interactive_common.sh"

ic_banner "Agent Hub (ARW) — Start Menu (macOS)" "Start services, tools, and debugging"
ic_project_overview
ic_feature_matrix
ic_host_summary

PORT=${ARW_PORT:-8091}
DEBUG=${ARW_DEBUG:-0}
DOCS_URL=${ARW_DOCS_URL:-}
ADMIN_TOKEN=${ARW_ADMIN_TOKEN:-}
USE_DIST=0
CFG_PATH=${ARW_CONFIG:-}
# Health wait defaults (can be toggled in runtime settings)
WAIT_HEALTH=${ARW_WAIT_HEALTH:-1}
WAIT_HEALTH_TIMEOUT_SECS=${ARW_WAIT_HEALTH_TIMEOUT_SECS:-20}

# Ensure local bin PATH for any helper installs
ic_path_add_local_bin

# Load persisted preferences if any
ic_env_load

RUN_DIR="$ROOT/.arw/run"
PID_FILE="$RUN_DIR/arw-server.pid"
mkdir -p "$RUN_DIR" && ic_log_dir_rel ".arw/run"
LOGS_DIR="$ROOT/.arw/logs"; mkdir -p "$LOGS_DIR" && ic_log_dir_rel ".arw/logs"

http_get() { # $1=url
  if command -v curl >/dev/null 2>&1; then curl -fsS "$1"; elif command -v wget >/dev/null 2>&1; then wget -qO- "$1"; else printf 'missing curl/wget' >&2; return 1; fi
}

preflight() {
  ic_section "Preflight"
  local svc
  svc=$(ic_detect_bin arw-server)
  if [[ ! -x "$svc" ]]; then
    if ! command -v cargo >/dev/null 2>&1; then
      ic_warn "No arw-server binary and Rust not installed. Attempting rustup…"
      ic_ensure_rust || { ic_err "Rust install failed; cannot build. Use portable dist/ bundle if available."; return 1; }
    fi
    ic_info "Building arw-server (release)"
    (cd "$ROOT" && ic_cargo build --release -p arw-server) || { ic_err "Build failed"; return 1; }
  fi
}

pick_config() {
  ic_section "Config"
  printf "Current ARW_CONFIG: %s\n" "${CFG_PATH:-<default configs/default.toml>}"
  read -r -p "Enter config path (or empty for default): " ans
  CFG_PATH=${ans:-$CFG_PATH}
}

configure_runtime() {
  ic_section "Runtime Settings"
  read -r -p "HTTP port [${PORT}]: " ans; PORT=${ans:-$PORT}
  if ic_port_in_use "$PORT"; then
    local np; np=$(ic_next_free_port "$PORT")
    ic_warn "Port $PORT busy. Suggesting $np"
    read -r -p "Use $np instead? (Y/n) " yn; [[ "${yn,,}" == n* ]] || PORT="$np"
  fi
  read -r -p "Enable debug endpoints? (y/N) " ans; [[ "${ans,,}" == y* ]] && DEBUG=1 || DEBUG=0
  read -r -p "Docs URL (optional) [${DOCS_URL}]: " ans; DOCS_URL=${ans:-$DOCS_URL}
  if [[ -z "${ADMIN_TOKEN}" ]]; then
    read -r -p "Generate admin token now? (Y/n): " gen; if [[ "${gen,,}" != n* ]]; then
      ADMIN_TOKEN=$(ic_generate_token)
      ic_emit_token "$ADMIN_TOKEN" "admin token"
      ic_warn "Store this token securely; it gates admin routes."
    fi
  fi
  read -r -p "Admin token [${ADMIN_TOKEN:-auto}]: " ans; ADMIN_TOKEN=${ans:-$ADMIN_TOKEN}
  read -r -p "Use packaged dist/ bundle when present? (y/N) " ans; [[ "${ans,,}" == y* ]] && USE_DIST=1 || USE_DIST=0
  read -r -p "Wait for /healthz after start? (Y/n) [$( [[ ${WAIT_HEALTH:-1} -eq 1 ]] && echo Y || echo n)]: " ans; [[ "${ans,,}" == n* ]] && WAIT_HEALTH=0 || WAIT_HEALTH=1
  read -r -p "Health wait timeout secs [${WAIT_HEALTH_TIMEOUT_SECS}]: " ans; WAIT_HEALTH_TIMEOUT_SECS=${ans:-$WAIT_HEALTH_TIMEOUT_SECS}
}

env_args() {
  local a=()
  [[ -n "$PORT" ]] && a+=(--port "$PORT")
  [[ $DEBUG -eq 1 ]] && a+=(--debug)
  [[ -n "$DOCS_URL" ]] && a+=(--docs-url "$DOCS_URL")
  [[ -n "$ADMIN_TOKEN" ]] && a+=(--admin-token "$ADMIN_TOKEN")
  [[ $USE_DIST -eq 1 ]] && a+=(--dist)
  [[ ${WAIT_HEALTH:-1} -eq 1 ]] && a+=(--wait-health --wait-health-timeout-secs "$WAIT_HEALTH_TIMEOUT_SECS")
  printf '%s\n' "${a[@]}"
}

start_service_only() {
  ic_section "Start: service only"
  if ! security_preflight; then ic_warn "Start canceled"; return; fi
  ARW_NO_LAUNCHER=1 ARW_NO_TRAY=1 ARW_PORT="$PORT" ARW_DOCS_URL="$DOCS_URL" ARW_ADMIN_TOKEN="$ADMIN_TOKEN" \
  ARW_CONFIG="$CFG_PATH" ARW_PID_FILE="$PID_FILE" ARW_LOG_FILE="$LOGS_DIR/arw-server.out.log" \
  readarray -t _ARGS < <(env_args); bash "$DIR/start.sh" "${_ARGS[@]}" || true
}

start_launcher_plus_service() {
  ic_section "Start: launcher + service"
  if ! security_preflight; then ic_warn "Start canceled"; return; fi
  ARW_PORT="$PORT" ARW_DOCS_URL="$DOCS_URL" ARW_ADMIN_TOKEN="$ADMIN_TOKEN" \
  ARW_CONFIG="$CFG_PATH" ARW_PID_FILE="$PID_FILE" ARW_LOG_FILE="$LOGS_DIR/arw-server.out.log" \
  readarray -t _ARGS < <(env_args); bash "$DIR/start.sh" "${_ARGS[@]}" || true
  # If launcher missing, hint a build
  local launcher
  launcher=$(ic_detect_bin arw-launcher)
  if [[ ! -x "$launcher" ]]; then
    ic_warn "Launcher not available. Build with: cargo build -p arw-launcher"
  fi
}

start_connector() {
  ic_section "Start: connector (if built with NATS)"
  local exe
  exe=$(ic_detect_bin arw-connector)
  if [[ -x "$exe" ]]; then
    ic_info "Launching $exe"
    "$exe" &
    disown || true
  else
    ic_warn "arw-connector not found (build with NATS features)"
  fi
  ic_press_enter
}

open_links_menu() {
  local base="http://127.0.0.1:$PORT"
  while true; do
    ic_banner "Open / Probe" "$base"
    cat <<EOF
  1) Open Debug UI (/admin/debug)
  2) Open API Spec (/spec)
  3) Open Tools JSON (/admin/tools)
  4) Curl health (/healthz)
  5) Check NATS connectivity
  6) Copy Debug URL to clipboard
  7) Copy Spec URL to clipboard
  8) Copy admin curl (tools)
  9) Copy admin curl (shutdown)
  0) Back
EOF
    read -r -p "Select: " pick || true
    case "$pick" in
      1) ic_open_url "$base/admin/debug" ;;
      2) ic_open_url "$base/spec" ;;
      3) ic_open_url "$base/admin/tools" ;;
      4) http_get "$base/healthz" || true; echo; ic_press_enter ;;
      5) read -r -p "NATS URL [nats://127.0.0.1:4222]: " u; u=${u:-nats://127.0.0.1:4222}; read -r h p < <(ic_parse_host_port "$u"); if ic_port_test "$h" "$p"; then ic_info "NATS reachable at $h:$p"; else ic_warn "Cannot reach $h:$p"; fi; ic_press_enter ;;
      6) ic_clipboard_copy "$base/admin/debug"; ic_info "Copied $base/admin/debug"; ic_press_enter ;;
      7) ic_clipboard_copy "$base/spec"; ic_info "Copied $base/spec"; ic_press_enter ;;
      8) {
           local tok="$ADMIN_TOKEN"; if [[ -z "$tok" ]]; then
            read -r -p "No token set. Generate one now? (Y/n): " g; if [[ "${g,,}" != n* ]]; then tok=$(ic_generate_token); export ARW_ADMIN_TOKEN="$tok"; ic_emit_token "$tok" "admin token (session)"; ic_warn "Store this token securely."; fi
           fi
           local cmd
           if [[ -n "$tok" ]]; then
             cmd="curl -sS -H 'X-ARW-Admin: $tok' '$base/admin/tools' | jq ."
           else
             cmd="curl -sS -H 'X-ARW-Admin: YOUR_TOKEN' '$base/admin/tools' | jq ."
           fi
           ic_clipboard_copy "$cmd"; ic_info "Copied admin curl snippet"; echo "$cmd"; ic_press_enter;
         } ;;
      9) {
           local tok="$ADMIN_TOKEN"; if [[ -z "$tok" ]]; then
            read -r -p "No token set. Generate one now? (Y/n): " g; if [[ "${g,,}" != n* ]]; then tok=$(ic_generate_token); export ARW_ADMIN_TOKEN="$tok"; ic_emit_token "$tok" "admin token (session)"; ic_warn "Store this token securely."; fi
           fi
           local cmd
           if [[ -n "$tok" ]]; then
             cmd="curl -sS -H 'X-ARW-Admin: $tok' '$base/shutdown'"
           else
             cmd="curl -sS -H 'X-ARW-Admin: YOUR_TOKEN' '$base/shutdown'"
           fi
           ic_clipboard_copy "$cmd"; ic_info "Copied admin curl shutdown"; echo "$cmd"; ic_press_enter;
         } ;;
      0|'') break ;;
      *) : ;;
    esac
  done
}

save_prefs_from_start() {
  export ARW_PORT="$PORT"
  export ARW_DOCS_URL="${DOCS_URL:-}"
  export ARW_ADMIN_TOKEN="${ADMIN_TOKEN:-}"
  export ARW_CONFIG="${CFG_PATH:-}"
  export ARW_WAIT_HEALTH="${WAIT_HEALTH:-1}"
  export ARW_WAIT_HEALTH_TIMEOUT_SECS="${WAIT_HEALTH_TIMEOUT_SECS:-20}"
  ic_env_save
}

force_stop() {
  ic_section "Force stop"
  if [[ -f "$PID_FILE" ]]; then
    local pid; pid=$(cat "$PID_FILE" 2>/dev/null || true)
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" || true
      sleep 0.3
      if kill -0 "$pid" 2>/dev/null; then
        kill -9 "$pid" || true
      fi
      ic_info "Stopped PID $pid"
    else
      ic_warn "No running PID found in $PID_FILE"
    fi
  else
    pkill -f "arw-server" || true
    ic_warn "PID file missing; attempted pkill arw-server"
  fi
  ic_press_enter
}

build_test_menu() {
  while true; do
    ic_banner "Build & Test" "Workspace targets"
    cat <<EOF
  1) Cargo build (release)
  2) Cargo build with NATS features (release)
  3) Cargo test (nextest)
  4) Generate docs page (docgen)
  5) Package portable bundle (dist/)
  0) Back
EOF
    read -r -p "Select: " pick || true
    case "$pick" in
      1) (cd "$ROOT" && ic_cargo build --workspace --release) ;;
      2) (cd "$ROOT" && ic_cargo build --workspace --release --features nats) ;;
      3) (cd "$ROOT" && ic_cargo nextest run --workspace) || true ;;
      4) bash "$DIR/docgen.sh" || ic_warn "docgen failed" ;;
      5) bash "$DIR/package.sh" || ic_warn "package failed" ;;
      0|'') break ;;
      *) : ;;
    esac
  done
}

cli_tools_menu() {
  local exe
  exe=$(ic_detect_bin arw-cli)
  if [[ ! -x "$exe" ]]; then
    ic_warn "arw-cli not found; build the workspace first"
    ic_press_enter; return
  fi
  while true; do
    ic_banner "CLI Tools" "$exe"
    cat <<EOF
  1) List tools (JSON)
  2) Print capsule template
  3) Generate ed25519 keypair (b64)
  0) Back
EOF
    read -r -p "Select: " pick || true
    case "$pick" in
      1) "$exe" tools || true; ic_press_enter ;;
      2) "$exe" capsule template || true; ic_press_enter ;;
      3) "$exe" capsule gen-ed25519 || true; ic_press_enter ;;
      0|'') break ;;
      *) : ;;
    esac
  done
}

main_menu() {
  preflight || true
  while true; do
    ic_banner "Start Menu" "Port=$PORT Debug=$DEBUG Dist=$USE_DIST HealthWait=$WAIT_HEALTH/${WAIT_HEALTH_TIMEOUT_SECS}s Nix=${ARW_USE_NIX:-0}"
    cat <<EOF
  1) Configure runtime (port/docs/token)
  2) Select config file (ARW_CONFIG)
  3) Start service only
  4) Start launcher + service
  5) Start connector (NATS)
  6) Open/probe endpoints
  7) Build & test
  8) CLI tools
  9) Stop service (/shutdown)
  10) Force stop (kill by PID/name)
  12) NATS manager (install/start/stop)
  11) Generate LaunchAgent (user)
  13) Open terminal here
  14) View logs
  15) Save preferences
  16) Doctor (quick checks)
  17) Configure HTTP port (write config)
  18) Spec sync (validate /spec)
  19) Docs build + open
  20) Launcher build check
  21) Export OpenAPI + schemas
  22) Security tips
  23) Troubleshoot (analyze logs)
  24) Start Caddy reverse proxy (https://localhost:8443)
  25) Stop Caddy reverse proxy
  26) Start Nginx reverse proxy (http://localhost:8080)
  27) Stop Nginx reverse proxy
  28) Disable debug now
  29) TLS wizard (LE/mkcert/self-signed)
  30) Start Caddy with selected Caddyfile
  31) Write session summary (./.arw/support)
  32) Stop all (svc/proxy/nats)
  33) Configure external base URL (write config)
  34) Validate+start proxy then test (/healthz,/spec)
  35) Audit supply-chain (cargo-audit/deny)
  0) Exit
EOF
    read -r -p "Select: " pick || true
    case "$pick" in
      1) configure_runtime ;;
      2) pick_config ;;
      3) start_service_only ;;
      4) start_launcher_plus_service ;;
      5) start_connector ;;
      6) open_links_menu ;;
      7) build_test_menu ;;
      8) cli_tools_menu ;;
      9) curl -fsS "http://127.0.0.1:$PORT/shutdown" || true; echo; ic_press_enter ;;
      10) force_stop ;;
      11) gen_launchagent ;;
      12) nats_menu ;;
      13) ic_open_terminal_here ;;
      14) logs_menu ;;
      15) save_prefs_from_start ;;
      16) ic_doctor ;;
      17) configure_http_port ;;
      18) spec_sync ;;
      19) docs_build_open ;;
      20) launcher_build_check ;;
      21) spec_export ;;
      22) ic_security_tips ;;
      23) troubleshoot ;;
      24) reverse_proxy_caddy_start ;;
      25) reverse_proxy_caddy_stop ;;
      26) reverse_proxy_nginx_start ;;
      27) reverse_proxy_nginx_stop ;;
      28) DEBUG=0; unset ARW_DEBUG; ic_info "Debug disabled for this session." ;;
      29) tls_wizard ;;
      30) reverse_proxy_caddy_choose_start ;;
      31) session_summary ;;
      32) stop_all ;;
      33) configure_external_base_url ;;
      34) proxy_validate_and_test ;;
      35) bash "$DIR/audit.sh" --interactive || ic_warn "audit helper failed" ;;
      0|'') break ;;
      *) : ;;
    esac
  done
}

gen_launchagent() {
  ic_section "LaunchAgent (user)"
  local svc
  svc=$(ic_detect_bin arw-server)
  if [[ -z "$svc" ]]; then ic_warn "Service binary not found; build first"; ic_press_enter; return; fi
  local plist_dir="$HOME/Library/LaunchAgents"
  mkdir -p "$plist_dir"
  local plist="$plist_dir/com.arw.svc.plist"
  local logs="$ROOT/.arw/logs"
  mkdir -p "$logs" && ic_log_dir_rel ".arw/logs"
  cat > "$plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>com.arw.svc</string>
  <key>ProgramArguments</key>
  <array>
    <string>$svc</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>ARW_PORT</key><string>$PORT</string>
    <key>ARW_CONFIG</key><string>${CFG_PATH}</string>
    <key>ARW_DOCS_URL</key><string>${DOCS_URL}</string>
    <key>ARW_ADMIN_TOKEN</key><string>${ADMIN_TOKEN}</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>$logs/arw-server.out.log</string>
  <key>StandardErrorPath</key><string>$logs/arw-server.err.log</string>
</dict>
</plist>
PLIST
  ic_info "Wrote $plist"
  echo "Run: launchctl load -w $plist"
  ic_press_enter
}

nats_menu() {
  while true; do
    ic_banner "NATS Manager" "Local broker under .arw/nats"
    cat <<EOF
  1) Install local NATS (no admin)
  2) Start NATS at nats://127.0.0.1:4222
  3) Stop NATS
  4) Check connectivity
  5) Configure NATS URL in configs/local.toml and set ARW_CONFIG
  0) Back
EOF
    read -r -p "Select: " pick || true
    case "$pick" in
      1) ic_nats_install || ic_warn "install failed" ;;
      2) ic_nats_start "nats://127.0.0.1:4222" || true ;;
      3) ic_nats_stop || true ;;
      4) if ic_port_test 127.0.0.1 4222; then ic_info "NATS reachable"; else ic_warn "NATS not reachable"; fi; ic_press_enter ;;
      5) configure_nats_url ;;
      0|'') break ;;
      *) : ;;
    esac
  done
}

configure_nats_url() {
  ic_section "Configure NATS in configs/local.toml"
  read -r -p "NATS URL [nats://127.0.0.1:4222]: " url; url=${url:-nats://127.0.0.1:4222}
  mkdir -p "$ROOT/configs"
  cat > "$ROOT/configs/local.toml" <<TOML
[runtime]
portable = true

[cluster]
enabled = true
bus = "nats"
queue = "nats"
nats_url = "$url"
TOML
  export ARW_CONFIG="$ROOT/configs/local.toml"
  ic_info "Wrote $ROOT/configs/local.toml and set ARW_CONFIG"
  ic_press_enter
}

configure_http_port() {
  ic_section "Configure HTTP port in configs/local.toml"
  read -r -p "HTTP port [${PORT}]: " p; p=${p:-$PORT}
  mkdir -p "$ROOT/configs"
  cat > "$ROOT/configs/local.toml" <<TOML
[runtime]
portable = true
port = $p

[cluster]
enabled = false
bus = "local"
queue = "local"
TOML
  export ARW_CONFIG="$ROOT/configs/local.toml"
  PORT="$p"
  ic_info "Wrote $ROOT/configs/local.toml and set ARW_CONFIG. Port=$p"
  ic_press_enter
}

spec_sync() {
  ic_section "Spec sync"
  local base="http://127.0.0.1:$PORT"
  if http_get "$base/spec" >/dev/null 2>&1; then ic_info "/spec ok"; else ic_warn "/spec not reachable"; fi
  if http_get "$base/spec/openapi.yaml" >/dev/null 2>&1; then ic_info "/spec/openapi.yaml ok"; else ic_warn "openapi not found"; fi
  if http_get "$base/healthz" >/dev/null 2>&1; then ic_info "/healthz ok"; else ic_warn "/healthz failed"; fi
  ic_open_url "$base/spec"
}

docs_build_open() {
  ic_section "Docs build"
  if [[ -x "$ROOT/.venv/bin/mkdocs" ]]; then PATH="$ROOT/.venv/bin:$PATH" ic_run mkdocs build || ic_warn "mkdocs build failed"; else ic_run mkdocs build || ic_warn "mkdocs build failed"; fi
  if [[ -f "$ROOT/site/index.html" ]]; then ic_open_url "file://$ROOT/site/index.html"; else ic_warn "site/index.html not found"; fi
}

launcher_build_check() {
  ic_section "Launcher build check (Tauri)"
  local log="$ROOT/.arw/logs/launcher-build.log"; mkdir -p "$(dirname "$log")"
  (cd "$ROOT" && ic_cargo build --release -p arw-launcher) &>"$log" || true
  tail -n 40 "$log" 2>/dev/null || true
  local exe="$ROOT/target/release/arw-launcher"; if [[ -x "$exe" ]]; then ic_info "Launcher built at $exe"; else ic_warn "Launcher build did not produce binary (see log)."; fi
  ic_press_enter
}

spec_export() {
  ic_section "Export OpenAPI + schemas"
  local svc; svc=$(ic_detect_bin arw-server)
  if [[ ! -x "$svc" ]]; then ic_warn "arw-server not built"; return; fi
  mkdir -p "$ROOT/spec"
  OPENAPI_OUT="$ROOT/spec/openapi.yaml" "$svc" || true
  ic_info "Wrote $ROOT/spec/openapi.yaml and schemas under spec/schemas if supported"
  ic_open_url "file://$ROOT/spec/openapi.yaml"
}

troubleshoot() {
  ic_banner "Troubleshooter" "Common issues"
  local tlog="$ROOT/.arw/logs/launcher-build.log"; if [[ -f "$tlog" ]]; then
    if grep -qi 'error' "$tlog"; then ic_warn "Launcher build errors detected. Open the log for details (.arw/logs/launcher-build.log)."; fi
  fi
  local slog="$ROOT/.arw/logs/arw-server.out.log"; if [[ -f "$slog" ]]; then
    if grep -qi 'failed to bind' "$slog"; then ic_warn "Port in use; configure a new port."; fi
    if grep -qi 'nats queue unavailable' "$slog"; then ic_warn "NATS unreachable; start NATS (NATS manager)."; fi
  fi
  ic_press_enter
}

logs_menu() {
  local logs="$ROOT/.arw/logs"; mkdir -p "$logs"
  local svc_log="$logs/arw-server.out.log"
  local nats_out="$logs/nats-server.out.log"
  local nats_err="$logs/nats-server.err.log"
  ic_banner "Logs" "$logs"
  echo "  1) Tail service log (if available)"
  echo "  2) Tail nats-server out"
  echo "  3) Tail nats-server err"
  echo "  0) Back"
  read -r -p "Select: " pick || true
  case "$pick" in
    1) ARW_LOG_FILE="$svc_log" ARW_NO_LAUNCHER=1 ARW_NO_TRAY=1 ARW_PORT="$PORT" bash -lc true >/dev/null 2>&1; if [[ -f "$svc_log" ]]; then tail -n 200 -f "$svc_log"; else { ic_warn "No service log yet. It will appear after next start with logging."; ic_press_enter; }; fi ;;
    2) if [[ -f "$nats_out" ]]; then tail -n 200 -f "$nats_out"; else { ic_warn "No nats out log"; ic_press_enter; }; fi ;;
    3) if [[ -f "$nats_err" ]]; then tail -n 200 -f "$nats_err"; else { ic_warn "No nats err log"; ic_press_enter; }; fi ;;
    *) : ;;
  esac
}

security_preflight() {
  if [[ ${DEBUG:-0} -eq 1 && -z "${ADMIN_TOKEN:-}" ]]; then
    ic_banner "Security Preflight" "Admin token recommended"
    echo "  ARW_DEBUG=1 enables admin endpoints without a token."
    echo "  Recommended: generate a token for this session or disable debug."
    echo "   1) Generate token and continue"
    echo "   2) Disable debug and continue"
    echo "   3) Continue without token (development)"
    echo "   4) Cancel start"
    read -r -p "Select [1/2/3]: " s; s=${s:-1}
    case "$s" in
      1) ADMIN_TOKEN=$(ic_rand_token); export ARW_ADMIN_TOKEN="$ADMIN_TOKEN"; ic_info "Token set for this session.";
         read -r -p "Persist token to .arw/env.sh? (Y/n): " sv; [[ "${sv,,}" != n* ]] && ic_env_save; return 0;;
      2) DEBUG=0; unset ARW_DEBUG; return 0;;
      3) return 0;;
      *) return 1;;
    esac
  fi
  return 0
}

reverse_proxy_templates() {
  ic_section "Reverse proxy templates"
  local out="$ROOT/configs/reverse_proxy"; mkdir -p "$out/nginx" "$out/caddy"
  # Caddyfile
  cat > "$out/caddy/Caddyfile" <<CADDY
localhost:8443 {
  tls internal
  reverse_proxy 127.0.0.1:$PORT
}
CADDY
  # Nginx config (HTTP and optional HTTPS with self-signed)
  cat > "$out/nginx/arw.conf" <<NGINX
upstream arw_upstream { server 127.0.0.1:$PORT; }
server {
  listen 8080;
  location / { proxy_pass http://arw_upstream; proxy_set_header Host \$host; proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for; }
}
# To enable TLS locally, generate a self-signed cert and enable the block below
# server {
#   listen 8443 ssl;
#   ssl_certificate     $out/nginx/certs/arw.local.crt;
#   ssl_certificate_key $out/nginx/certs/arw.local.key;
#   location / { proxy_pass http://arw_upstream; proxy_set_header Host \$host; proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for; }
# }
NGINX
  # Offer to generate self-signed certs
  if command -v openssl >/dev/null 2>&1; then
    read -r -p "Generate self-signed cert for nginx? (y/N): " yn
    if [[ "${yn,,}" == y* ]]; then
      mkdir -p "$out/nginx/certs"
      openssl req -x509 -newkey rsa:2048 -nodes -keyout "$out/nginx/certs/arw.local.key" -out "$out/nginx/certs/arw.local.crt" -subj "/CN=localhost" -days 365 2>/dev/null || true
      ic_info "Wrote self-signed certs under $out/nginx/certs"
    fi
  fi
  ic_info "Caddyfile: $out/caddy/Caddyfile"
  ic_info "Nginx:     $out/nginx/arw.conf"
  echo "Run Caddy (if installed): caddy run --config $out/caddy/Caddyfile"
  echo "Run nginx (if installed): sudo nginx -c $out/nginx/arw.conf (may need to merge into nginx.conf)"
  ic_press_enter
}

reverse_proxy_caddy_start() {
  ic_section "Start Caddy reverse proxy"
  if ! command -v caddy >/dev/null 2>&1; then
    ic_warn "caddy not found. Install caddy (brew install caddy | see https://caddyserver.com)"
    return
  fi
  local cfg="$ROOT/configs/reverse_proxy/caddy/Caddyfile"
  [[ -f "$cfg" ]] || reverse_proxy_templates
  mkdir -p "$LOGS_DIR"
  caddy run --config "$cfg" >"$LOGS_DIR/caddy.out.log" 2>&1 &
  echo $! > "$RUN_DIR/caddy.pid"
  ic_info "Caddy started (pid $(cat "$RUN_DIR/caddy.pid")) — open https://localhost:8443"
  ic_open_url "https://localhost:8443"
}

reverse_proxy_caddy_stop() {
  ic_section "Stop Caddy"
  local pidf="$RUN_DIR/caddy.pid"
  if [[ -f "$pidf" ]]; then
    local pid; pid=$(cat "$pidf" 2>/dev/null || true)
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
    rm -f "$pidf"
    ic_info "Stopped Caddy"
  else
    pkill -f "caddy run --config" 2>/dev/null || true
    ic_warn "No Caddy pid file; attempted pkill"
  fi
}

reverse_proxy_caddy_choose_start() {
  ic_section "Start Caddy with selected Caddyfile"
  local dir="$ROOT/configs/reverse_proxy/caddy"; mkdir -p "$dir"
  local files=()
  while IFS= read -r -d '' f; do files+=("$f"); done < <(find "$dir" -maxdepth 1 -type f -name 'Caddyfile*' -print0 2>/dev/null)
  if [[ ${#files[@]} -eq 0 ]]; then ic_warn "No Caddyfiles found; run TLS wizard or generate templates"; return; fi
  local i=1; for f in "${files[@]}"; do echo "  $i) $(basename "$f")"; i=$((i+1)); done
  read -r -p "Select: " sel; sel=${sel:-1}; local idx=$((sel-1));
  local cfg="${files[$idx]}"; [[ -f "$cfg" ]] || { ic_warn "Invalid selection"; return; }
  if ! ic_caddy_validate "$cfg"; then
    read -r -p "Validation failed. Start anyway? (y/N): " yn; [[ "${yn,,}" == y* ]] || return
  fi
  mkdir -p "$LOGS_DIR"
  caddy run --config "$cfg" >"$LOGS_DIR/caddy.out.log" 2>&1 &
  echo $! > "$RUN_DIR/caddy.pid"
  ic_info "Caddy started with $(basename "$cfg") — https://localhost:8443"
  ic_open_url "https://localhost:8443"
}

session_summary() {
  ic_section "Session summary"
  local sup="$ROOT/.arw/support"; mkdir -p "$sup"; ic_log_dir_rel ".arw/support"
  local ts; ts=$(date +%Y%m%d_%H%M%S)
  local out="$sup/session_$ts.md"
  local nats="${ARW_NATS_URL:-nats://127.0.0.1:4222}"; read -r h p < <(ic_parse_host_port "$nats")
  {
    echo "# ARW Session Summary ($ts)"
    echo "- Port: $PORT"
    echo "- Debug: ${DEBUG:-0}"
    echo "- ARW_CONFIG: ${CFG_PATH:-<default>}"
    echo "- Docs URL: ${DOCS_URL:-}"
    echo "- Admin token set: $([[ -n "${ADMIN_TOKEN:-}" ]] && echo yes || echo no)"
    echo "- NATS reachable ($nats): $([[ $(ic_port_test "$h" "$p"; echo $?) -eq 0 ]] && echo yes || echo no)"
    echo "- Service log: $LOGS_DIR/arw-server.out.log"
    echo "- Caddy running: $([[ -f "$RUN_DIR/caddy.pid" ]] && echo yes || echo no)"
    echo "- Nginx pid file: $([[ -f "$RUN_DIR/nginx.pid" ]] && echo "$RUN_DIR/nginx.pid" || echo none)"
    echo
    echo "## Useful URLs"
    echo "- Debug: http://127.0.0.1:$PORT/admin/debug"
    echo "- Spec:  http://127.0.0.1:$PORT/spec"
  } > "$out"
  ic_info "Wrote $out"
}

stop_all() {
  ic_section "Stop all"
  force_stop || true
  reverse_proxy_caddy_stop || true
  reverse_proxy_nginx_stop || true
  ic_nats_stop || true
  ic_info "Stopped svc/proxy/nats"
}

configure_external_base_url() {
  ic_section "External base URL"
  read -r -p "External base URL (e.g., https://arw.example.com) [${ARW_EXTERNAL_BASE_URL:-}]: " u
  [[ -z "$u" ]] && { ic_warn "No URL provided"; return; }
  if [[ ! "$u" =~ ^https?:// ]]; then ic_warn "Must start with http:// or https://"; return; fi
  mkdir -p "$ROOT/configs"
  cat > "$ROOT/configs/local.toml" <<TOML
[runtime]
portable = true
port = ${PORT}
external_base_url = "$u"

[cluster]
enabled = false
bus = "local"
queue = "local"
TOML
  export ARW_CONFIG="$ROOT/configs/local.toml"; export ARW_EXTERNAL_BASE_URL="$u"
  ic_info "Wrote $ROOT/configs/local.toml with external_base_url and set ARW_CONFIG"
}

proxy_validate_and_test() {
  ic_section "Validate+Start proxy then test"
  local dir="$ROOT/configs/reverse_proxy/caddy"; mkdir -p "$dir"
  local cfg="$dir/Caddyfile"
  if [[ ! -f "$cfg" ]]; then
    ic_warn "Default Caddyfile not found; using TLS wizard or choose-start to pick one"
    reverse_proxy_caddy_choose_start || return
  else
    if ! ic_caddy_validate "$cfg"; then ic_warn "Validation failed"; return; fi
    reverse_proxy_caddy_start || true
  fi
  ic_info "Testing proxy endpoints"
  ic_test_proxy "https://localhost:8443/healthz" || true
  ic_test_proxy "https://localhost:8443/spec" || true
}

reverse_proxy_nginx_start() {
  ic_section "Start Nginx reverse proxy"
  if ! command -v nginx >/dev/null 2>&1; then ic_warn "nginx not found"; return; fi
  local out="$ROOT/configs/reverse_proxy/nginx"; [[ -f "$out/arw.conf" ]] || reverse_proxy_templates
  mkdir -p "$out/logs"
  nginx -p "$out" -c arw.conf -g "pid $RUN_DIR/nginx.pid;" || ic_warn "nginx start failed"
  ic_info "Nginx started (pid file: $RUN_DIR/nginx.pid) — open http://localhost:8080"
  ic_open_url "http://localhost:8080"
}

reverse_proxy_nginx_stop() {
  ic_section "Stop Nginx"
  local pidf="$RUN_DIR/nginx.pid"
  if [[ -f "$pidf" ]]; then
    local pid; pid=$(cat "$pidf" 2>/dev/null || true)
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null || true
    rm -f "$pidf" 2>/dev/null || true
    ic_info "Stopped Nginx"
  else
    pkill -f "nginx: master process" 2>/dev/null || true
    ic_warn "No Nginx pid file; attempted pkill"
  fi
}

tls_wizard() {
  ic_banner "TLS Wizard" "Choose a TLS strategy"
  echo "  1) Public domain with Let's Encrypt (Caddy)"
  echo "  2) Local dev TLS via mkcert (Caddy)"
  echo "  3) Self-signed (already available)"
  read -r -p "Select [1/2/3]: " t; t=${t:-3}
  local outc="$ROOT/configs/reverse_proxy/caddy"; mkdir -p "$outc"
  case "$t" in
    1)
      read -r -p "Domain (e.g., arw.example.com): " d
      read -r -p "Email for ACME (e.g., you@example.com): " e
      if [[ -z "$d" || -z "$e" ]]; then ic_warn "Domain and email required"; return; fi
      cat > "$outc/Caddyfile.$d" <<CADDY
$d {
  tls $e
  reverse_proxy 127.0.0.1:$PORT
}
CADDY
      ic_info "Wrote $outc/Caddyfile.$d"
      echo "Note: ensure ports 80/443 are reachable and DNS resolves $d to this host."
      ;;
    2)
      if ! command -v mkcert >/dev/null 2>&1; then ic_warn "mkcert not found (brew install mkcert). Falling back to self-signed."; return; fi
      read -r -p "Dev hostname (default: localhost): " d; d=${d:-localhost}
      local cert="$outc/$d.crt" key="$outc/$d.key"
      mkcert -install || true
      mkcert -cert-file "$cert" -key-file "$key" "$d"
      cat > "$outc/Caddyfile.$d" <<CADDY
$d {
  tls $cert $key
  reverse_proxy 127.0.0.1:$PORT
}
CADDY
      ic_info "Wrote $outc/Caddyfile.$d with mkcert certs"
      ;;
    *)
      ic_info "Self-signed is supported: use Caddyfile with 'tls internal' (already generated)."
      ;;
  esac
}

main_menu
