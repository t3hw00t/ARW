#!/usr/bin/env bash
set -euo pipefail

# Shared helpers for interactive scripts (Linux/macOS)

_ic_color() { # $1=color $2=msg
  local c="$1"; shift || true
  case "$c" in
    cyan) printf "\033[36m%s\033[0m" "$*";;
    green) printf "\033[32m%s\033[0m" "$*";;
    yellow) printf "\033[33m%s\033[0m" "$*";;
    red) printf "\033[31m%s\033[0m" "$*";;
    magenta) printf "\033[35m%s\033[0m" "$*";;
    bold) printf "\033[1m%s\033[0m" "$*";;
    *) printf "%s" "$*";;
  esac
}

ic_banner() {
  local title="$1"
  local subtitle="${2:-}"
  local cols
  cols=$(tput cols 2>/dev/null || echo 80)
  local line
  line=$(printf '%*s' "$cols" '')
  line=${line// /━}
  printf '\n%s\n' "$line"
  printf " %s\n" "$(_ic_color bold "$title")"
  if [[ -n "$subtitle" ]]; then printf " %s\n" "$(_ic_color cyan "$subtitle")"; fi
  printf '%s\n\n' "$line"
}

ic_section() { printf "\n%s %s\n" "$(_ic_color magenta '▶')" "$(_ic_color bold "$*")"; }
ic_info()    { printf "%s %s\n" "$(_ic_color cyan '[info]')" "$*"; }
ic_warn()    { printf "%s %s\n" "$(_ic_color yellow '[warn]')" "$*"; }
ic_err()     { printf "%s %s\n" "$(_ic_color red '[error]')" "$*"; }

ic_press_enter() { read -r -p "Press Enter to continue…" _; }

ic_root() { (cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd); }
ic_local_bin_dir() { echo "$(ic_root)/.arw/bin"; }
ic_path_add_local_bin() {
  local b; b=$(ic_local_bin_dir)
  mkdir -p "$b"
  ic_log_dir_rel ".arw/bin"
  case ":$PATH:" in
    *":$b:"*) : ;;
    *) export PATH="$b:$PATH" ;;
  esac
}

ic_detect_os() {
  case "$(uname -s)" in
    Linux*) echo linux;;
    Darwin*) echo macos;;
    *) echo unknown;;
  esac
}

ic_has_nix() { command -v nix >/dev/null 2>&1; }
# Default: prefer Nix dev shell if available
: "${ARW_USE_NIX:=}"
if [[ -z "${ARW_USE_NIX}" ]]; then
  if ic_has_nix; then ARW_USE_NIX=1; else ARW_USE_NIX=0; fi
fi

# Allow system package managers? Off by default to avoid host/global changes.
: "${ARW_ALLOW_SYSTEM_PKGS:=0}"

# Install log helpers
ic_install_log_path() { echo "$(ic_root)/.install.log"; }
ic_log_dir_rel() { # $1=relative path from root
  local rel="$1"; local log; log=$(ic_install_log_path)
  { [[ -f "$log" ]] || echo "# Install log - $(date)" > "$log"; } 2>/dev/null || true
  grep -qxF "DIR $rel" "$log" 2>/dev/null || printf 'DIR %s\n' "$rel" >> "$log"
}

# Persisted env (project-local)
ic_env_file() { echo "$(ic_root)/.arw/env.sh"; }
ic_env_load() {
  local f; f=$(ic_env_file)
  if [[ -f "$f" ]]; then
    # shellcheck disable=SC1090
    . "$f"
  fi
}
ic_env_save() { # save known keys to .arw/env.sh
  local root; root=$(ic_root)
  mkdir -p "$root/.arw"
  ic_log_dir_rel ".arw"
  local f; f=$(ic_env_file)
  {
    echo "# ARW env (project-local). Source this file to apply preferences."
    echo "export ARW_USE_NIX=${ARW_USE_NIX:-0}"
    echo "export ARW_ALLOW_SYSTEM_PKGS=${ARW_ALLOW_SYSTEM_PKGS:-0}"
    echo "export ARW_PORT=${ARW_PORT:-8090}"
    echo "export ARW_DOCS_URL=${ARW_DOCS_URL:-}"
    echo "export ARW_ADMIN_TOKEN=${ARW_ADMIN_TOKEN:-}"
    echo "export ARW_CONFIG=${ARW_CONFIG:-}"
    echo "export ARW_NATS_URL=${ARW_NATS_URL:-}"
    echo "export ARW_WAIT_HEALTH=${ARW_WAIT_HEALTH:-1}"
    echo "export ARW_WAIT_HEALTH_TIMEOUT_SECS=${ARW_WAIT_HEALTH_TIMEOUT_SECS:-20}"
  } > "$f"
  ic_info "Saved preferences to $f"
}

ic_detect_pm() {
  # Echo one of: apt dnf yum pacman zypper apk brew none
  if command -v apt-get >/dev/null 2>&1; then echo apt
  elif command -v dnf >/dev/null 2>&1; then echo dnf
  elif command -v yum >/dev/null 2>&1; then echo yum
  elif command -v pacman >/dev/null 2>&1; then echo pacman
  elif command -v zypper >/dev/null 2>&1; then echo zypper
  elif command -v apk >/dev/null 2>&1; then echo apk
  elif command -v brew >/dev/null 2>&1; then echo brew
  else echo none; fi
}

ic_install_pkg() { # $1=pkg symbolic name
  # Best-effort package install for common managers.
  # Supports symbolic names: jq, pkg-config, gtk-dev
  local sym="$1"; shift || true
  local pm; pm=$(ic_detect_pm)
  local pkgs=()
  case "$sym" in
    jq)
      case "$pm" in
        apt) pkgs=(jq);;
        dnf) pkgs=(jq);;
        yum) pkgs=(jq);;
        pacman) pkgs=(jq);;
        zypper) pkgs=(jq);;
        apk) pkgs=(jq);;
        brew) pkgs=(jq);;
      esac
      ;;
    pkg-config)
      case "$pm" in
        apt) pkgs=(pkg-config);;
        dnf) pkgs=(pkgconf-pkg-config);;
        yum) pkgs=(pkgconfig);;
        pacman) pkgs=(pkgconf);;
        zypper) pkgs=(pkg-config);;
        apk) pkgs=(pkgconf);;
        brew) pkgs=(pkg-config);;
      esac
      ;;
    gtk-dev)
      case "$pm" in
        apt) pkgs=(libgtk-3-dev);;
        dnf) pkgs=(gtk3-devel);;
        yum) pkgs=(gtk3-devel);;
        pacman) pkgs=(gtk3);;
        zypper) pkgs=(gtk3-devel);;
        apk) pkgs=(gtk+3.0-dev);;
        brew) pkgs=(gtk+3);;
      esac
      ;;
  esac
  if [[ "${#pkgs[@]}" -eq 0 || "$pm" == none ]]; then
    ic_warn "No supported package manager found for '$sym'"
    return 1
  fi
  if [[ "${ARW_ALLOW_SYSTEM_PKGS}" != "1" ]]; then
    ic_warn "System package installs disabled. To enable, toggle 'Allow system package managers' in Dependencies."
    ic_info "Manual command example: $pm install ${pkgs[*]}"
    return 1
  fi
  ic_info "Installing ${pkgs[*]} via $pm"
  case "$pm" in
    apt) sudo apt-get update && sudo apt-get install -y "${pkgs[@]}" ;;
    dnf) sudo dnf install -y "${pkgs[@]}" ;;
    yum) sudo yum install -y "${pkgs[@]}" ;;
    pacman) sudo pacman -Sy --noconfirm "${pkgs[@]}" ;;
    zypper) sudo zypper install -y "${pkgs[@]}" ;;
    apk) sudo apk add --no-cache "${pkgs[@]}" ;;
    brew) brew install "${pkgs[@]}" ;;
  esac
}

ic_ensure_rust() {
  # Prefer a project-local Rust install
  local root; root=$(ic_root)
  export RUSTUP_HOME="$root/.arw/rust/rustup"
  export CARGO_HOME="$root/.arw/rust/cargo"
  mkdir -p "$RUSTUP_HOME" "$CARGO_HOME"
  ic_log_dir_rel ".arw/rust"
  export PATH="$CARGO_HOME/bin:$PATH"
  if command -v cargo >/dev/null 2>&1; then return 0; fi
  ic_warn "Rust toolchain not found. Attempting rustup install (project-local)."
  if command -v curl >/dev/null 2>&1; then
    curl https://sh.rustup.rs -sSf | sh -s -- -y --no-modify-path
  elif command -v wget >/dev/null 2>&1; then
    wget -qO- https://sh.rustup.rs | sh -s -- -y --no-modify-path
  else
    ic_err "Neither curl nor wget found; cannot install rustup automatically."
    return 1
  fi
  export PATH="$CARGO_HOME/bin:$PATH"
  command -v cargo >/dev/null 2>&1 || return 1
}

ic_ensure_nextest() {
  if ! command -v cargo >/dev/null 2>&1; then return 1; fi
  if cargo nextest --version >/dev/null 2>&1; then return 0; fi
  ic_info "Installing cargo-nextest (in user cargo bin)"
  cargo install --locked cargo-nextest || return 1
}

ic_ensure_jq() {
  if command -v jq >/dev/null 2>&1; then return 0; fi
  ic_warn "jq not found. Trying package manager or local download."
  if ic_install_pkg jq; then return 0; fi
  # Attempt local download of jq into .arw/bin (Linux x86_64 and macOS arm64/amd64)
  ic_path_add_local_bin
  local os arch url out
  os=$(ic_detect_os)
  arch=$(uname -m)
  out="$(ic_local_bin_dir)/jq"
  if [[ "$os" == linux && "$arch" == x86_64 ]]; then
    url="https://github.com/jqlang/jq/releases/download/jq-1.6/jq-linux64"
  elif [[ "$os" == linux && "$arch" == aarch64 ]]; then
    url="https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-linux-arm64"
  elif [[ "$os" == macos && "$arch" == arm64 ]]; then
    url="https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-macos-arm64"
  elif [[ "$os" == macos && "$arch" == x86_64 ]]; then
    url="https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-macos-amd64"
  else
    url=""
  fi
  if [[ -n "$url" ]]; then
    ic_info "Downloading jq to local bin ($out)"
    if command -v curl >/dev/null 2>&1; then curl -L "$url" -o "$out"; elif command -v wget >/dev/null 2>&1; then wget -O "$out" "$url"; else ic_err "Need curl/wget to fetch jq"; return 1; fi
    chmod +x "$out" || true
    # Print checksum for manual verification
    if command -v sha256sum >/dev/null 2>&1; then
      local sum; sum=$(sha256sum "$out" | awk '{print $1}')
      ic_info "jq sha256: $sum (verify at: https://github.com/jqlang/jq/releases)"
    elif command -v shasum >/dev/null 2>&1; then
      local sum; sum=$(shasum -a 256 "$out" | awk '{print $1}')
      ic_info "jq sha256: $sum (verify at: https://github.com/jqlang/jq/releases)"
    fi
    command -v jq >/dev/null 2>&1 && return 0
  fi
  ic_warn "jq install failed; docs generation may be limited"
  return 1
}

ic_ensure_mkdocs_venv() {
  # Create a local venv under .venv and install mkdocs stack if possible
  if command -v mkdocs >/dev/null 2>&1; then return 0; fi
  if ! command -v python3 >/dev/null 2>&1; then
    ic_warn "python3 not found; cannot create local MkDocs venv"
    return 1
  fi
  local root venv py pip
  root=$(ic_root)
  venv="$root/.venv"
  python3 -m venv "$venv" || { ic_warn "venv creation failed"; return 1; }
  # shellcheck disable=SC1090
  . "$venv/bin/activate"
  pip install --upgrade pip || true
  pip install mkdocs mkdocs-material mkdocs-git-revision-date-localized-plugin || return 1
  ic_log_dir_rel ".venv"
  return 0
}

# Nix env wrappers
ic_run() { # run command, optionally via nix dev shell
  if [[ "${ARW_USE_NIX}" == "1" ]] && ic_has_nix; then
    nix develop --command "$@"
  else
    "$@"
  fi
}

ic_cargo() { # proxy cargo through nix devshell or local rust
  ic_ensure_rust || true
  if [[ "${ARW_USE_NIX}" == "1" ]] && ic_has_nix; then
    nix develop --command cargo "$@"
  else
    cargo "$@"
  fi
}

# Ports
ic_port_in_use() { # $1=port
  local p="$1"
  # Try bash /dev/tcp
  if (echo > "/dev/tcp/127.0.0.1/$p") >/dev/null 2>&1; then return 0; fi
  # ss or lsof fallback
  if command -v ss >/dev/null 2>&1; then ss -ltn 2>/dev/null | awk '{print $4}' | grep -q ":$p$" && return 0; fi
  if command -v lsof >/dev/null 2>&1; then lsof -iTCP:"$p" -sTCP:LISTEN >/dev/null 2>&1 && return 0; fi
  return 1
}
ic_next_free_port() { # $1=start
  local p=${1:-8090}
  local limit=$((p+100))
  while [[ $p -lt $limit ]]; do
    if ! ic_port_in_use "$p"; then echo "$p"; return 0; fi
    p=$((p+1))
  done
  echo "$1" # fallback
}

# NATS helpers
ic_parse_host_port() { # $1=url -> echo host port
  local url="$1" rest host port
  rest=${url#*://}
  host=${rest%%:*}
  port=${rest##*:}
  [[ "$host" == "$rest" ]] && host="$rest" && port="4222"
  echo "$host $port"
}
ic_port_test() { # $1=host $2=port
  local h="$1" p="$2"
  (exec 3<>"/dev/tcp/$h/$p") >/dev/null 2>&1
}

# NATS local install/run (no admin)
ic_nats_dir() { echo "$(ic_root)/.arw/nats"; }
ic_nats_bin() {
  local d; d=$(ic_nats_dir)
  if [[ "$(ic_detect_os)" == macos || "$(ic_detect_os)" == linux ]]; then
    echo "$d/nats-server"
  else
    echo "$d/nats-server.exe"
  fi
}
ic_fetch() { # $1=url $2=out
  local url="$1" out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -L "$url" -o "$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$out" "$url"
  else
    return 1
  fi
}
ic_nats_install() { # [$1=version]
  local ver="${1:-2.10.19}" # default pinned
  local os arch ext asset url tmp outdir bin
  os=$(ic_detect_os)
  case "$os" in
    linux) os=linux; ext=tar.gz;;
    macos) os=darwin; ext=tar.gz;;
    *) os=windows; ext=zip;;
  esac
  local m; m=$(uname -m)
  if [[ "$m" == aarch64 || "$m" == arm64 ]]; then arch=arm64; else arch=amd64; fi
  asset="nats-server-v${ver}-${os}-${arch}.${ext}"
  url="https://github.com/nats-io/nats-server/releases/download/v${ver}/${asset}"
  outdir="$(ic_root)/.arw/nats"
  mkdir -p "$outdir/tmp" && ic_log_dir_rel ".arw/nats"
  tmp="$outdir/tmp/$asset"
  ic_info "Fetching $asset"
  if ! ic_fetch "$url" "$tmp"; then
    ic_warn "Download failed: $url"
    ic_info "Fallback suggestion: docker run -p 4222:4222 nats:latest"
    return 1
  fi
  if [[ "$ext" == tar.gz ]]; then
    tar -xzf "$tmp" -C "$outdir/tmp" || { ic_err "tar extraction failed"; return 1; }
  else
    if command -v unzip >/dev/null 2>&1; then unzip -q -o "$tmp" -d "$outdir/tmp"; else ic_err "unzip not found"; return 1; fi
  fi
  bin=$(find "$outdir/tmp" -type f -name 'nats-server*' | head -n1 || true)
  if [[ -z "$bin" ]]; then ic_err "nats-server binary not found in archive"; return 1; fi
  if [[ "$os" == windows ]]; then
    cp "$bin" "$outdir/nats-server.exe"
  else
    cp "$bin" "$outdir/nats-server" && chmod +x "$outdir/nats-server"
  fi
  ic_info "Installed nats-server to $outdir"
  # Print checksum
  if command -v sha256sum >/dev/null 2>&1; then
    local sum; sum=$(sha256sum "$tmp" | awk '{print $1}')
    ic_info "archive sha256: $sum"
  elif command -v shasum >/dev/null 2>&1; then
    local sum; sum=$(shasum -a 256 "$tmp" | awk '{print $1}')
    ic_info "archive sha256: $sum"
  fi
}
ic_nats_start() { # [$1=url]
  local url="${1:-nats://127.0.0.1:4222}"
  local h p; read -r h p < <(ic_parse_host_port "$url")
  local bin; bin=$(ic_nats_bin)
  if [[ ! -x "$bin" ]]; then ic_warn "nats-server not installed; run Install local NATS in Dependencies"; return 1; fi
  local run="$(ic_root)/.arw/run"; mkdir -p "$run" && ic_log_dir_rel ".arw/run"
  local logs="$(ic_root)/.arw/logs"; mkdir -p "$logs" && ic_log_dir_rel ".arw/logs"
  nohup "$bin" -a 127.0.0.1 -p "$p" >"$logs/nats-server.out.log" 2>"$logs/nats-server.err.log" &
  echo $! > "$run/nats-server.pid"
  ic_info "nats-server started at 127.0.0.1:$p (pid $(cat "$run/nats-server.pid"))"
}
ic_nats_stop() {
  local run="$(ic_root)/.arw/run"
  if [[ -f "$run/nats-server.pid" ]]; then
    local pid; pid=$(cat "$run/nats-server.pid" 2>/dev/null || true)
    if [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null; then
      kill "$pid" || true; sleep 0.3; kill -9 "$pid" 2>/dev/null || true
      ic_info "Stopped nats-server PID $pid"
      return 0
    fi
  fi
  pkill -f "nats-server" 2>/dev/null || true
  ic_warn "PID file missing; attempted pkill nats-server"
}

# Open terminal here (Linux/macOS best-effort)
ic_open_terminal_here() {
  local root; root=$(ic_root)
  if command -v x-terminal-emulator >/dev/null 2>&1; then x-terminal-emulator -e bash -lc "cd '$root'; exec bash" & disown; return; fi
  if command -v gnome-terminal >/dev/null 2>&1; then gnome-terminal --working-directory="$root" & disown; return; fi
  if command -v konsole >/dev/null 2>&1; then konsole --workdir "$root" & disown; return; fi
  if command -v xfce4-terminal >/dev/null 2>&1; then xfce4-terminal --working-directory="$root" & disown; return; fi
  if command -v kitty >/dev/null 2>&1; then kitty -d "$root" & disown; return; fi
  if command -v wezterm >/dev/null 2>&1; then wezterm start --cwd "$root" & disown; return; fi
  if command -v xterm >/dev/null 2>&1; then xterm -e bash -lc "cd '$root'; exec bash" & disown; return; fi
  if [[ $(ic_detect_os) == macos ]]; then
    if command -v osascript >/dev/null 2>&1; then
      osascript <<OSA
tell application "Terminal"
  do script "cd '$root'"
  activate
end tell
OSA
      return
    fi
  fi
  ic_warn "Could not launch a terminal automatically."
}

# Quick doctor: summarize env and common pitfalls
ic_doctor() {
  ic_section "Doctor"
  command -v cargo >/dev/null 2>&1 && ic_info "cargo: $(cargo --version)" || ic_warn "cargo not found"
  command -v jq >/dev/null 2>&1 && ic_info "jq: $(jq --version)" || ic_warn "jq not found"
  command -v mkdocs >/dev/null 2>&1 && ic_info "mkdocs: $(mkdocs --version 2>/dev/null | head -n1)" || ic_warn "mkdocs not found (docs optional)"
  local svc; svc=$(ic_detect_bin arw-svc); [[ -x "$svc" ]] && ic_info "arw-svc: $svc" || ic_warn "arw-svc not built"
  local tray; tray=$(ic_detect_bin arw-tray); [[ -x "$tray" ]] && ic_info "arw-tray: $tray" || ic_warn "tray not built (GTK optional)"
  local nats="${ARW_NATS_URL:-nats://127.0.0.1:4222}"; read -r h p < <(ic_parse_host_port "$nats");
  if ic_port_test "$h" "$p"; then ic_info "NATS reachable at $h:$p"; else ic_warn "NATS not reachable at $h:$p"; fi
}

# Clipboard copy helper (best-effort)
ic_clipboard_copy() { # $1=text
  local t="$1"
  if command -v xclip >/dev/null 2>&1; then printf '%s' "$t" | xclip -selection clipboard; return
  elif command -v xsel >/dev/null 2>&1; then printf '%s' "$t" | xsel --clipboard --input; return
  elif command -v pbcopy >/dev/null 2>&1; then printf '%s' "$t" | pbcopy; return
  fi
}

# Security tips
ic_security_tips() {
  ic_banner "Security Tips" "Protect admin endpoints"
  cat <<TIPS
  • Sensitive endpoints: /debug, /probe, /memory/*, /models/*, /governor/*, /introspect/*, /chat/*, /feedback/*
  • In development, set ARW_DEBUG=1 to simplify. In production, disable it.
  • Set ARW_ADMIN_TOKEN and send header: X-ARW-Admin: <token>
  • Rate limiting for admin endpoints can be adjusted via ARW_ADMIN_RL (default 60/60).
  • Consider putting the service behind a reverse proxy with TLS in multi-user environments.
TIPS
}

# Proxy configuration helper
ic_configure_proxy() {
  ic_section "Configure HTTP(S) proxy"
  read -r -p "HTTP_PROXY [${HTTP_PROXY:-}]: " hp; hp=${hp:-${HTTP_PROXY:-}}
  read -r -p "HTTPS_PROXY [${HTTPS_PROXY:-}]: " sp; sp=${sp:-${HTTPS_PROXY:-}}
  read -r -p "NO_PROXY (comma-separated) [${NO_PROXY:-}]: " np; np=${np:-${NO_PROXY:-}}
  [[ -n "$hp" ]] && export HTTP_PROXY="$hp" && export http_proxy="$hp"
  [[ -n "$sp" ]] && export HTTPS_PROXY="$sp" && export https_proxy="$sp"
  [[ -n "$np" ]] && export NO_PROXY="$np" && export no_proxy="$np"
  ic_info "Proxy env updated for this session."
  read -r -p "Persist to .arw/env.sh? (Y/n): " ans; if [[ "${ans,,}" != n* ]]; then ic_env_save; fi
}

# Validate Caddy config (dry-run)
ic_caddy_validate() { # $1=path
  local cfg="$1"
  if ! command -v caddy >/dev/null 2>&1; then ic_warn "caddy not found"; return 1; fi
  caddy validate --config "$cfg" && { ic_info "Caddyfile valid: $(basename "$cfg")"; return 0; } || {
    ic_warn "Caddyfile validation failed for $(basename "$cfg")"
    return 1
  }
}

# Test reverse proxy URL (200 expected)
ic_test_proxy() { # $1=url
  local url="$1"
  if command -v curl >/dev/null 2>&1; then
    local code; code=$(curl -k -s -o /dev/null -w "%{http_code}" "$url")
    [[ "$code" == "200" ]] && { ic_info "OK $url"; return 0; } || { ic_warn "BAD ($code) $url"; return 1; }
  elif command -v wget >/dev/null 2>&1; then
    wget --no-check-certificate -q "$url" -O /dev/null && { ic_info "OK $url"; return 0; } || { ic_warn "BAD $url"; return 1; }
  else
    ic_warn "No curl/wget to test $url"; return 1
  fi
}


ic_open_url() {
  local url="$1"
  if command -v xdg-open >/dev/null 2>&1; then xdg-open "$url" >/dev/null 2>&1 || true
  elif command -v open >/dev/null 2>&1; then open "$url" >/dev/null 2>&1 || true
  else
    ic_info "Open this URL in your browser: $url"
  fi
}

ic_host_summary() {
  local os; os=$(ic_detect_os)
  ic_section "Host / Hardware"
  if [[ "$os" == linux ]]; then
    local dist kernel cpu cores mem disk gpu
    if [[ -f /etc/os-release ]]; then
      # shellcheck disable=SC1091
      . /etc/os-release
      dist="$NAME $VERSION"
    else dist="Linux"; fi
    kernel=$(uname -r)
    cpu=$(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2- | sed 's/^ *//')
    cores=$(nproc 2>/dev/null || echo "-")
    mem=$(free -h 2>/dev/null | awk '/Mem:/ {print $2}')
    disk=$(df -h . | awk 'NR==2{print $4" free"}')
    gpu=$(command -v lspci >/dev/null 2>&1 && lspci | grep -E 'VGA|3D' | head -n1 | cut -d: -f3- | sed 's/^ *//' || echo 'N/A')
    printf "  • OS:        %s (kernel %s)\n" "$dist" "$kernel"
    printf "  • CPU:       %s (%s cores)\n" "${cpu:-N/A}" "$cores"
    printf "  • Memory:    %s\n" "${mem:-N/A}"
    printf "  • Disk:      %s\n" "${disk:-N/A}"
    printf "  • GPU:       %s\n" "$gpu"
  elif [[ "$os" == macos ]]; then
    local prod ver kernel cpu cores mem_bytes mem_human disk
    prod=$(sw_vers -productName 2>/dev/null || echo macOS)
    ver=$(sw_vers -productVersion 2>/dev/null || echo "-")
    kernel=$(uname -r)
    cpu=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || echo "-")
    cores=$(sysctl -n hw.ncpu 2>/dev/null || echo "-")
    mem_bytes=$(sysctl -n hw.memsize 2>/dev/null || echo 0)
    if [[ "$mem_bytes" =~ ^[0-9]+$ ]]; then
      mem_human=$(awk -v m="$mem_bytes" 'BEGIN{printf "%.1f GB", m/1024/1024/1024}')
    else mem_human="-"; fi
    disk=$(df -h / | awk 'NR==2{print $4" free"}')
    printf "  • OS:        %s %s (kernel %s)\n" "$prod" "$ver" "$kernel"
    printf "  • CPU:       %s (%s cores)\n" "$cpu" "$cores"
    printf "  • Memory:    %s\n" "$mem_human"
    printf "  • Disk:      %s\n" "$disk"
  else
    printf "  • OS:        %s\n" "$(uname -a)"
  fi
}

ic_project_overview() {
  ic_section "Project"
  printf "  %s\n" "Agents Running Wild (ARW) — local-first Rust workspace for personal AI agents."
  printf "  %s\n" "Highlights: user-mode HTTP service + debug UI; macro-driven tools;"
  printf "             event stream + tracing hooks; portable packaging."
}

ic_feature_matrix() {
  ic_section "Feature Matrix"
  printf "  • Service:    arw-svc (HTTP, /debug UI)\n"
  printf "  • CLI:        arw-cli (tools, capsules, gates)\n"
  printf "  • Tray:       arw-tray (optional; Linux requires GTK)\n"
  printf "  • Connector:  arw-connector (optional; NATS feature)\n"
  printf "  • Docs:       MkDocs site under docs/ (optional)\n"
}

ic_detect_bin() { # $1=bin-name (arw-svc|arw-cli|...)
  local root; root=$(ic_root)
  local exe="$1"; [[ "${OS:-}" == "Windows_NT" ]] && exe+=".exe"
  local path="$root/target/release/$exe"
  [[ -x "$path" ]] && { echo "$path"; return; }
  local base
  base=$(ls -td "$root"/dist/arw-* 2>/dev/null | head -n1 || true)
  if [[ -n "$base" && -x "$base/bin/$exe" ]]; then echo "$base/bin/$exe"; return; fi
  echo "" # not found
}
# Generate a random admin token
ic_rand_token() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 16
  elif command -v dd >/dev/null 2>&1; then
    dd if=/dev/urandom bs=16 count=1 2>/dev/null | xxd -p -c 32
  else
    date +%s%N | sha256sum | cut -c1-32 2>/dev/null || echo "arw$(date +%s)"
  fi
}

# Create a support bundle with logs/configs (redacted env)
ic_support_bundle() {
  local root; root=$(ic_root)
  local outdir="$root/.arw/support"; mkdir -p "$outdir" && ic_log_dir_rel ".arw/support"
  local ts; ts=$(date +%Y%m%d_%H%M%S)
  local name="arw_support_${ts}.tar.gz"
  local tmp="$outdir/tmp_$ts"; mkdir -p "$tmp"
  mkdir -p "$tmp/logs" "$tmp/configs"
  [[ -d "$root/.arw/logs" ]] && cp -r "$root/.arw/logs/." "$tmp/logs/" || true
  [[ -d "$root/configs" ]] && cp -r "$root/configs/." "$tmp/configs/" || true
  # Redacted env
  {
    echo "# Redacted ARW env"
    env | grep '^ARW_' | sed -E 's/(ARW_ADMIN_TOKEN=).*/\1***REDACTED***/' || true
  } > "$tmp/env_redacted.txt"
  (cd "$tmp/.." && tar -czf "$name" "$(basename "$tmp")")
  rm -rf "$tmp"
  ic_info "Support bundle: $outdir/$name"
}
