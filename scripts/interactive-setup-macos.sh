#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"
# shellcheck source=lib/interactive_common.sh
. "$DIR/lib/interactive_common.sh"

ic_banner "ARW — Interactive Setup (macOS)" "Portable, local-first agent workspace"
ic_project_overview
ic_feature_matrix
ic_host_summary

echo
printf " %s\n" "$(_ic_color bold 'Before we start…')"
printf "  This guided setup will:\n"
printf "  - Check prerequisites (Rust, Python/MkDocs)\n"
printf "  - Build the workspace (release) and optionally run tests\n"
printf "  - Optionally generate docs and package a portable bundle\n"
printf "  - Optionally help configure clustering (NATS)\n\n"

PORT=${ARW_PORT:-8090}
DOCS_URL=${ARW_DOCS_URL:-}
ADMIN_TOKEN=${ARW_ADMIN_TOKEN:-}
RUN_TESTS=0
BUILD_DOCS=1
DO_PACKAGE=0
USE_NIX=${ARW_USE_NIX:-0}
HEADLESS=${ARW_HEADLESS:-0}

# Ensure local bin in PATH for on-the-fly installs
ic_path_add_local_bin

# Args
while [[ $# -gt 0 ]]; do
  case "$1" in
    --headless) HEADLESS=1; shift;;
    --package) DO_PACKAGE=1; shift;;
    --no-docs) BUILD_DOCS=0; shift;;
    *) shift;;
  esac
done

ensure_prereqs() {
  ic_section "Prerequisites"
  local ok=1
  if ! command -v cargo >/dev/null 2>&1; then
    ok=0; ic_warn "Rust 'cargo' not found. You can install it from the Dependencies menu."
  else ic_info "cargo: $(cargo --version)"; fi
  if ! command -v python3 >/dev/null 2>&1; then
    ic_warn "python3 not found (docs optional)"
  fi
  if command -v mkdocs >/dev/null 2>&1; then
    ic_info "mkdocs: $(mkdocs --version 2>/dev/null | head -n1)"
  else
    ic_warn "MkDocs not found (docs optional)"
  fi
  ic_press_enter
  return $ok
}

install_mkdocs() {
  ic_section "Install MkDocs (optional)"
  if command -v python3 >/dev/null 2>&1; then
    python3 -m pip install --user --upgrade pip || true
    python3 -m pip install --user mkdocs mkdocs-material mkdocs-git-revision-date-localized-plugin || true
    command -v mkdocs >/dev/null 2>&1 && ic_info "MkDocs installed." || ic_warn "MkDocs install may have failed"
  else
    ic_warn "python3 not found; cannot install MkDocs"
  fi
  ic_press_enter
}

dependencies_menu() {
  while true; do
    ic_banner "Dependencies" "Install or enable common prerequisites"
    cat <<EOF
  1) Install Rust toolchain (rustup)
  2) Install cargo-nextest (tests)
  3) Install jq (brew or local)
  4) Install pkg-config (for tray)
  5) Install GTK (for tray)
  6) Create local MkDocs venv (.venv)
  7) Toggle use of Nix devshell for builds [current: ${ARW_USE_NIX}]
  8) Toggle system package managers (brew/apt) [current: ${ARW_ALLOW_SYSTEM_PKGS:-0}]
  9) Install local NATS (no admin)
  10) Configure HTTP(S) proxy
  0) Back
EOF
    read -r -p "Select: " pick || true
    case "$pick" in
      1) ic_ensure_rust || ic_warn "Rust install failed or unavailable" ;;
      2) ic_ensure_nextest || ic_warn "cargo-nextest install failed" ;;
      3) ic_ensure_jq || ic_warn "jq install failed" ;;
      4) ic_install_pkg pkg-config || ic_warn "pkg-config install failed" ;;
      5) ic_install_pkg gtk-dev || ic_warn "GTK install failed" ;;
      6) ic_ensure_mkdocs_venv || ic_warn "MkDocs venv install failed" ;;
      7) if [[ "${ARW_USE_NIX}" == "1" ]]; then export ARW_USE_NIX=0; else export ARW_USE_NIX=1; fi ;;
      8) if [[ "${ARW_ALLOW_SYSTEM_PKGS:-0}" == "1" ]]; then export ARW_ALLOW_SYSTEM_PKGS=0; else export ARW_ALLOW_SYSTEM_PKGS=1; fi ;;
      9) ic_nats_install || nats_guidance ;;
      10) ic_configure_proxy ;;
      0|'') break ;;
      *) : ;;
    esac
  done
}

configure_settings() {
  ic_section "Configure Settings"
  read -r -p "HTTP port [${PORT}]: " ans; PORT=${ans:-$PORT}
  read -r -p "Docs URL (optional) [${DOCS_URL}]: " ans; DOCS_URL=${ans:-$DOCS_URL}
  read -r -p "Admin token (optional) [${ADMIN_TOKEN}]: " ans; ADMIN_TOKEN=${ans:-$ADMIN_TOKEN}
  read -r -p "Run tests after build? (y/N): " ans; [[ "${ans,,}" == y* ]] && RUN_TESTS=1 || RUN_TESTS=0
  read -r -p "Build docs site with MkDocs? (Y/n): " ans; [[ "${ans,,}" == n* ]] && BUILD_DOCS=0 || BUILD_DOCS=1
  read -r -p "Create portable bundle in dist/? (y/N): " ans; [[ "${ans,,}" == y* ]] && DO_PACKAGE=1 || DO_PACKAGE=0
}

configure_cluster() {
  ic_section "Clustering (optional)"
  local enable url node
  read -r -p "Enable cluster with NATS? (y/N): " enable || true
  if [[ "${enable,,}" == y* ]]; then
    read -r -p "NATS URL [nats://127.0.0.1:4222]: " url; url=${url:-nats://127.0.0.1:4222}
    read -r -p "Node ID [$(hostname)]: " node; node=${node:-$(hostname)}
    mkdir -p "$ROOT/configs"
    cat > "$ROOT/configs/local.toml" <<TOML
[runtime]
portable = true

[cluster]
enabled = true
bus = "nats"
queue = "nats"
nats_url = "${url}"
node_id = "${node}"
TOML
    ic_info "Wrote configs/local.toml (set ARW_CONFIG=configs/local.toml when starting)"
  else
    ic_info "Keeping single-node defaults (configs/default.toml)"
  fi
  ic_press_enter
}

do_build() {
  ic_section "Build (release)"
  (cd "$ROOT" && ic_cargo build --workspace --release)
  if [[ $RUN_TESTS -eq 1 ]]; then
    ic_section "Tests"
    (cd "$ROOT" && ic_cargo nextest run --workspace) || true
  fi
}

do_docs() {
  if [[ $BUILD_DOCS -eq 1 ]]; then
    ic_section "Docs generation"
    ic_ensure_jq || ic_warn "jq unavailable; docgen may be limited"
    if [[ -x "$ROOT/.venv/bin/mkdocs" ]]; then export PATH="$ROOT/.venv/bin:$PATH"; fi
    bash "$DIR/docgen.sh" || ic_warn "docgen failed"
  fi
}

do_package() {
  if [[ $DO_PACKAGE -eq 1 ]]; then
    ic_section "Packaging portable bundle"
    bash "$DIR/package.sh" --no-build || ic_warn "package failed"
  fi
}

nats_guidance() {
  ic_section "NATS server (optional)"
  echo "  NATS powers clustering and the connector."
  echo "  Quick start options:"
  echo "   - Homebrew: brew install nats-server"
  echo "   - Docker: docker run -p 4222:4222 nats:latest"
  echo "   - Manual: https://github.com/nats-io/nats-server/releases"
  if [[ "${ARW_ALLOW_SYSTEM_PKGS:-0}" == "1" ]]; then
    ic_info "Attempting: brew install nats-server"
    brew install nats-server || true
  else
    ic_warn "System package managers are disabled. Enable in Dependencies to try brew."
  fi
  ic_press_enter
}

main_menu() {
  while true; do
    ic_banner "Setup Menu" "Choose an action"
    cat <<EOF
  1) Check prerequisites
  2) Dependencies (install/fix common requirements)
  3) Install MkDocs toolchain (optional)
  4) Configure settings (port/docs/token)
  5) Configure clustering (NATS)
  6) Build now (release)
  7) Generate docs (if enabled)
  8) Package portable bundle (dist/)
  9) Run everything (build → docs → package)
  10) Open documentation (local)
  11) Save preferences (./.arw/env.sh)
  12) Doctor (quick checks)
  13) First-run wizard (guided)
  14) Create support bundle
  0) Exit
EOF
    read -r -p "Select: " pick || true
    case "$pick" in
      1) ensure_prereqs ;;
      2) dependencies_menu ;;
      3) install_mkdocs ;;
      4) configure_settings ;;
      5) configure_cluster ;;
      6) do_build ;;
      7) do_docs ;;
      8) do_package ;;
      9) do_build; do_docs; do_package ;;
      10) if [[ -f "$ROOT/site/index.html" ]]; then ic_open_url "file://$ROOT/site/index.html"; else ic_open_url "file://$ROOT/docs/index.md"; fi ;;
      11) ic_env_save ;;
      12) ic_doctor ;;
      13) first_run_wizard ;;
      14) ic_support_bundle ;;
      0|'') break ;;
      *) : ;;
    esac
  done

  ic_section "Next"
  printf "  Start the service with: %s\n" "$(_ic_color bold "$DIR/interactive-start-macos.sh")"
}

first_run_wizard() {
  ic_banner "First‑Run Wizard" "Guided setup for ARW"
  ic_section "Goal"
  echo "  Choose a setup profile:"
  echo "   1) Local only (no tray)"
  echo "   2) Local with tray (optional)"
  echo "   3) Cluster (NATS)"
  read -r -p "Select [1/2/3]: " prof; prof=${prof:-1}

  ic_section "HTTP Port"
  read -r -p "Port [${PORT}]: " p; p=${p:-$PORT}
  if ic_port_in_use "$p"; then np=$(ic_next_free_port "$p"); ic_warn "Port busy; suggesting $np"; read -r -p "Use $np? (Y/n) " yn; [[ "${yn,,}" != n* ]] && p=$np; fi

  ic_section "Admin Token"
  read -r -p "Generate admin token? (Y/n): " gen; if [[ "${gen,,}" != n* ]]; then tok=$(ic_rand_token); export ARW_ADMIN_TOKEN="$tok"; ic_info "Generated token: $tok"; fi

  ic_section "Write config"
  mkdir -p "$ROOT/configs"
  if [[ "$prof" == 3 ]]; then
    read -r -p "NATS URL [nats://127.0.0.1:4222]: " nurl; nurl=${nurl:-nats://127.0.0.1:4222}
    cat > "$ROOT/configs/local.toml" <<TOML
[runtime]
portable = true
port = $p

[cluster]
enabled = true
bus = "nats"
queue = "nats"
nats_url = "$nurl"
TOML
  else
    cat > "$ROOT/configs/local.toml" <<TOML
[runtime]
portable = true
port = $p

[cluster]
enabled = false
bus = "local"
queue = "local"
TOML
  fi
  export ARW_CONFIG="$ROOT/configs/local.toml"; PORT="$p"
  ic_info "Wrote configs/local.toml and set ARW_CONFIG"

  if [[ "$prof" == 2 ]]; then
    ic_section "Tray"
    if command -v pkg-config >/dev/null 2>&1; then pkg-config --exists gtk+-3.0 && ic_info "GTK ok" || ic_warn "GTK dev missing (tray optional)"; else ic_warn "pkg-config missing (tray optional)"; fi
  elif [[ "$prof" == 3 ]]; then
    ic_section "NATS"
    ic_nats_install || ic_warn "NATS install failed"
    ic_nats_start "${nurl:-nats://127.0.0.1:4222}" || true
  fi

  ic_section "Build"
  (cd "$ROOT" && ic_cargo build --workspace --release) || ic_warn "build failed"

  ic_section "Start"
  read -r -p "Start service now? (Y/n): " go; if [[ "${go,,}" != n* ]]; then
    ARW_NO_TRAY=$([[ "$prof" == 2 ]] && echo 0 || echo 1) \
    ARW_PORT="$PORT" ARW_CONFIG="$ARW_CONFIG" ARW_DOCS_URL="$DOCS_URL" \
    ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" bash "$DIR/start.sh" --debug --port "$PORT" --wait-health --wait-health-timeout-secs 20
    ic_open_url "http://127.0.0.1:$PORT/spec"
  fi

  ic_section "Save"
  read -r -p "Save preferences to .arw/env.sh? (Y/n): " sv; [[ "${sv,,}" != n* ]] && ic_env_save
}

if [[ $HEADLESS -eq 1 ]]; then
  ensure_prereqs || true
  do_build
  do_docs
  DO_PACKAGE=${DO_PACKAGE:-1}
  do_package
  exit 0
fi

configure_settings # initial pass for convenience
main_menu
