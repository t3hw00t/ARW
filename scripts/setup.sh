#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"

WARNINGS=()
# Track what this setup installs so uninstall.sh can undo it later
INSTALL_LOG="$ROOT/.install.log"
echo "# Install log - $(date)" > "$INSTALL_LOG"
yes_flag=0
run_tests=0
no_docs=0
minimal=0
headless=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    -y|--yes) yes_flag=1; shift;;
    --no-docs) no_docs=1; shift;;
    --run-tests) run_tests=1; shift;;
    --minimal) minimal=1; no_docs=1; shift;;
    --headless) headless=1; shift;;
    *) echo "Unknown option: $1"; exit 1;;
  esac
done

title(){ echo -e "\033[36m\n=== $* ===\033[0m"; }
info(){ echo -e "\033[36m[setup]\033[0m $*"; }
warn(){ WARNINGS+=("$*"); }
pause(){ [[ $yes_flag -eq 1 ]] || read -rp "$*" _; }

launcher_runtime_ready=1
check_launcher_runtime() {
  if [[ "${OSTYPE:-}" != linux* ]]; then
    return 0
  fi
  if ! command -v pkg-config >/dev/null 2>&1; then
    warn "pkg-config not found; unable to verify WebKitGTK 4.1 + libsoup3 for the launcher. If the build fails, run scripts/install-tauri-deps.sh or see docs/guide/compatibility.md."
    launcher_runtime_ready=0
    return 0
  fi
  if pkg-config --exists webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0 >/dev/null 2>&1; then
    info "WebKitGTK 4.1 + libsoup3 detected (launcher build ready)."
    launcher_runtime_ready=1
  else
    warn "WebKitGTK 4.1 + libsoup3 development packages not detected. Run scripts/install-tauri-deps.sh or review docs/guide/compatibility.md before rebuilding the launcher."
    launcher_runtime_ready=0
  fi
}

build_launcher=1
if [[ $headless -eq 1 ]]; then
  build_launcher=0
fi

title "Prerequisites"
if ! command -v cargo >/dev/null 2>&1; then
  warn "Rust 'cargo' not found."
  echo "Install Rust via rustup: https://rustup.rs"
  pause "Press Enter after installing Rust (or Ctrl+C to abort)"
fi

if [[ $minimal -eq 1 ]]; then
  info "Minimal mode enabled: skipping docs toolchain, docgen, and packaging."
fi
if [[ $headless -eq 1 ]]; then
  info "Headless mode enabled: launcher build will be skipped."
fi

mkdocs_ok=0
if command -v mkdocs >/dev/null 2>&1; then mkdocs_ok=1; fi
if [[ $no_docs -eq 0 && $mkdocs_ok -eq 0 ]]; then
  if command -v python3 >/dev/null 2>&1; then
    info "Installing MkDocs via pip (user site)"
    if ! python3 -m pip --version >/dev/null 2>&1; then
      info "Bootstrapping pip in user site"
      if python3 -m ensurepip --upgrade --user >/dev/null 2>&1; then
        :
      else
        warn "Unable to bootstrap pip; docs site generation will be skipped unless MkDocs is installed manually."
      fi
    fi
    if python3 -m pip install --user mkdocs mkdocs-material mkdocs-git-revision-date-localized-plugin; then
      mkdocs_ok=1
      printf 'PIP_USER %s\n' mkdocs mkdocs-material mkdocs-git-revision-date-localized-plugin >> "$INSTALL_LOG"
      user_base="$(python3 -m site --user-base 2>/dev/null || true)"
      user_bin="${user_base:+$user_base/bin}"
      if [[ -n "$user_bin" && -d "$user_bin" && ":$PATH:" != *":$user_bin:"* ]]; then
        warn "MkDocs installed to $user_bin. Add it to PATH to run mkdocs directly."
      fi
    else
      warn "MkDocs install failed or pip is unavailable; docs site build will be skipped."
    fi
  else
    warn "python3 not found; skipping docs site build"
  fi
fi
if [[ $build_launcher -eq 1 ]]; then
  check_launcher_runtime
  if [[ $launcher_runtime_ready -eq 0 ]]; then
    warn "Launcher dependencies missing; continuing in headless mode."
    build_launcher=0
  fi
fi
title "Build workspace (release)"
if [[ $minimal -eq 1 ]]; then
  info "Building arw-server (release)"
  (cd "$ROOT" && cargo build --release --locked -p arw-server)
  info "Building arw-cli (release)"
  (cd "$ROOT" && cargo build --release --locked -p arw-cli)
  if [[ $build_launcher -eq 1 ]]; then
    info "Building arw-launcher (release)"
    if (cd "$ROOT" && cargo build --release --locked -p arw-launcher); then
      :
    else
      warn "arw-launcher build failed; continue in headless mode (install WebKitGTK 4.1 + libsoup3 or run with --headless)."
      build_launcher=0
    fi
  else
    info "Skipping arw-launcher build."
  fi
else
  info "Building workspace (release, excluding launcher)"
  (cd "$ROOT" && cargo build --workspace --release --locked --exclude arw-launcher)
  if [[ $build_launcher -eq 1 ]]; then
    info "Building arw-launcher (release)"
    if (cd "$ROOT" && cargo build --release --locked -p arw-launcher); then
      :
    else
      warn "arw-launcher build failed; continue in headless mode (install WebKitGTK 4.1 + libsoup3 or run with --headless)."
      build_launcher=0
    fi
  else
    info "Skipping arw-launcher build."
  fi
fi
printf 'DIR target\n' >> "$INSTALL_LOG"

if [[ $run_tests -eq 1 ]]; then
  title "Run tests (workspace)"
  cargo nextest run --workspace --locked
fi

if [[ $minimal -eq 0 ]]; then
  title "Generate workspace status page"
  if command -v jq >/dev/null 2>&1; then
    bash "$DIR/docgen.sh" || warn "docgen failed"
  else
    warn "jq not found; skipping docgen page generation (install: apt-get install jq | brew install jq)"
  fi

  title "Package portable bundle"
  bash "$DIR/package.sh" --no-build
  printf 'DIR dist\n' >> "$INSTALL_LOG"
  [[ -d "$ROOT/site" ]] && echo 'DIR site' >> "$INSTALL_LOG"
fi
if [[ ${#WARNINGS[@]} -gt 0 ]]; then
  title "Warnings"
  for w in "${WARNINGS[@]}"; do
    echo -e "\033[33m- $w\033[0m"
  done
fi
if [[ $minimal -eq 1 ]]; then
  info "Done. Core binaries are under target/release/."
else
  info "Done. See dist/ for portable bundle."
fi
