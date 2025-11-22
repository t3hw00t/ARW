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
with_launcher=0
skip_build=0
build_cli=1
cli_flag=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    -y|--yes) yes_flag=1; shift;;
    --no-docs) no_docs=1; shift;;
    --run-tests) run_tests=1; shift;;
    --minimal) minimal=1; no_docs=1; shift;;
    --headless) headless=1; shift;;
    --with-launcher)
      headless=0
      with_launcher=1
      shift;;
    --skip-build|--no-build) skip_build=1; shift;;
    --skip-cli)
      build_cli=0
      cli_flag="skip"
      shift
      ;;
    --with-cli)
      build_cli=1
      cli_flag="with"
      shift
      ;;
    *) echo "Unknown option: $1"; exit 1;;
  esac
done

if [[ "${ARW_SETUP_AGENT:-0}" == 1 && -z "$cli_flag" ]]; then
  build_cli=0
fi

title(){ echo -e "\033[36m\n=== $* ===\033[0m"; }
info(){ echo -e "\033[36m[setup]\033[0m $*"; }
warn(){ WARNINGS+=("$*"); }
pause(){ [[ $yes_flag -eq 1 ]] || read -rp "$*" _; }

VENV="${ARW_VENV:-$ROOT/.venv}"
PYTHON_BOOTSTRAP="$(command -v python3 || command -v python || true)"
PYTHON_BIN=""
build_mode="$(printf '%s' "${ARW_BUILD_MODE:-release}" | tr '[:upper:]' '[:lower:]')"
case "$build_mode" in
  release|debug) ;;
  *) build_mode="release" ;;
esac
build_label="$build_mode"

ensure_venv() {
  if [[ -n "$PYTHON_BIN" && -x "$PYTHON_BIN" ]]; then
    return 0
  fi
  if [[ -z "$PYTHON_BOOTSTRAP" ]]; then
    warn "python3 not found; unable to create venv at $VENV"
    return 1
  fi
  if [[ ! -d "$VENV" ]]; then
    info "Creating venv at $VENV"
    if ! "$PYTHON_BOOTSTRAP" -m venv "$VENV"; then
      warn "Failed to create venv at $VENV"
      return 1
    fi
  fi
  PYTHON_BIN="$VENV/bin/python"
  if [[ ! -x "$PYTHON_BIN" ]]; then
    warn "venv exists at $VENV but python binary missing"
    return 1
  fi
  "$PYTHON_BIN" -m ensurepip --upgrade >/dev/null 2>&1 || true
  "$PYTHON_BIN" -m pip install --upgrade pip >/dev/null 2>&1 || true
  return 0
}

ensure_python_module() {
  local module="$1"
  local package="${2:-$1}"
  if ! ensure_venv; then
    return 1
  fi
  if "$PYTHON_BIN" - <<PY >/dev/null 2>&1
import importlib
import sys
sys.exit(0 if importlib.util.find_spec("$module") else 1)
PY
  then
    return 0
  fi
  info "Installing Python module ${package} in venv ($VENV)"
  if "$PYTHON_BIN" -m pip install "${package}"; then
    printf 'VENV %s\n' "$package" >> "$INSTALL_LOG"
    return 0
  fi
  warn "Failed to install Python module ${package}; run '$PYTHON_BIN -m pip install ${package}' manually."
  return 1
}

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
if ensure_venv; then
  case ":$PATH:" in
    *":$VENV/bin:"*) : ;;
    *) PATH="$VENV/bin:$PATH"; export PATH ;;
  esac
else
  warn "Proceeding without repo venv; Python-based steps may be skipped."
fi

if [[ $minimal -eq 1 ]]; then
  info "Minimal mode enabled: skipping docs toolchain, docgen, and packaging."
fi
if [[ $headless -eq 1 ]]; then
  info "Headless mode enabled: launcher build will be skipped."
elif [[ $with_launcher -eq 1 ]]; then
  info "Launcher opt-in enabled: attempting Tauri launcher build."
fi
if [[ $skip_build -eq 1 ]]; then
  info "Skip-build enabled: workspace compile/test steps will be bypassed."
fi
if [[ "$build_mode" == "debug" ]]; then
  info "Debug build mode enabled: using cargo build --locked (no --release) for faster iteration."
fi

mkdocs_ok=0
if ensure_venv && "$PYTHON_BIN" - <<'PY' >/dev/null 2>&1
import importlib.util
import sys
mods = ["mkdocs", "mkdocs_material", "mkdocs_git_revision_date_localized_plugin"]
sys.exit(0 if all(importlib.util.find_spec(m) for m in mods) else 1)
PY
then
  mkdocs_ok=1
fi
if [[ $no_docs -eq 0 && $mkdocs_ok -eq 0 ]]; then
  if ensure_venv; then
    info "Installing MkDocs toolchain in venv ($VENV)"
    if "$PYTHON_BIN" -m pip install mkdocs mkdocs-material mkdocs-git-revision-date-localized-plugin; then
      mkdocs_ok=1
      printf 'VENV %s\n' mkdocs mkdocs-material mkdocs-git-revision-date-localized-plugin >> "$INSTALL_LOG"
    else
      warn "MkDocs install failed; docs site build will be skipped."
    fi
  else
    warn "python/venv unavailable; skipping docs site build"
  fi
fi
if [[ -n "$PYTHON_BIN" ]]; then
  ensure_python_module yaml pyyaml || true
fi
if [[ $build_launcher -eq 1 ]]; then
  check_launcher_runtime
  if [[ $launcher_runtime_ready -eq 0 ]]; then
    warn "Launcher dependencies missing; continuing in headless mode."
    build_launcher=0
  fi
fi
build_flags=(--locked)
if [[ "$build_mode" == "release" ]]; then
  build_flags+=(--release)
fi
title "Build workspace (${build_label})"
if [[ $skip_build -eq 1 ]]; then
  info "Skipping workspace build (--skip-build)."
else
  if [[ $minimal -eq 1 ]]; then
    info "Building arw-server (${build_label})"
    (cd "$ROOT" && cargo build "${build_flags[@]}" -p arw-server)
    if [[ $build_cli -eq 1 ]]; then
      info "Building arw-cli (${build_label})"
      (cd "$ROOT" && cargo build "${build_flags[@]}" -p arw-cli)
    else
      info "Skipping arw-cli build (requested)"
    fi
    if [[ $build_launcher -eq 1 ]]; then
      info "Building arw-launcher (${build_label})"
      if (cd "$ROOT" && cargo build "${build_flags[@]}" -p arw-launcher --features launcher-linux-ui); then
        :
      else
        warn "arw-launcher build failed; continue in headless mode (install WebKitGTK 4.1 + libsoup3 or run with --headless)."
        build_launcher=0
      fi
    else
      info "Skipping arw-launcher build."
    fi
  else
    if [[ $build_cli -eq 1 ]]; then
      info "Building workspace (${build_label}, excluding launcher)"
    else
      info "Building workspace (${build_label}, excluding launcher and arw-cli)"
    fi
    cargo_args=(build "${build_flags[@]}" --workspace --exclude arw-launcher)
    if [[ $build_cli -ne 1 ]]; then
      cargo_args+=(--exclude arw-cli)
    fi
    (cd "$ROOT" && cargo "${cargo_args[@]}")
    if [[ $build_launcher -eq 1 ]]; then
      info "Building arw-launcher (${build_label})"
      if (cd "$ROOT" && cargo build "${build_flags[@]}" -p arw-launcher --features launcher-linux-ui); then
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
fi

if [[ $run_tests -eq 1 ]]; then
  if [[ $skip_build -eq 1 ]]; then
    warn "--run-tests requested but build step was skipped; not running tests."
  else
    title "Run tests (workspace)"
    if command -v cargo-nextest >/dev/null 2>&1; then
      (cd "$ROOT" && cargo nextest run --workspace --locked --test-threads=1)
    else
      if command -v cargo >/dev/null 2>&1; then
        install_nextest=0
        if [[ $yes_flag -eq 1 ]]; then
          install_nextest=1
        else
          read -rp "cargo-nextest not found. Install now? (Y/n) " resp
          if [[ -z "$resp" || "$resp" =~ ^[Yy]$ ]]; then
            install_nextest=1
          fi
        fi
        if [[ $install_nextest -eq 1 ]]; then
          info "Installing cargo-nextest (cargo install --locked cargo-nextest)"
        if cargo install --locked cargo-nextest; then
            (cd "$ROOT" && cargo nextest run --workspace --locked --test-threads=1)
          else
            warn "cargo-nextest install failed; falling back to cargo test --workspace --locked."
            (cd "$ROOT" && cargo test --workspace --locked -- --test-threads=1)
          fi
        else
          warn "Skipping cargo-nextest install; falling back to cargo test --workspace --locked."
          (cd "$ROOT" && cargo test --workspace --locked -- --test-threads=1)
        fi
      else
        warn "cargo-nextest not found and cargo unavailable; running cargo test --workspace --locked."
        (cd "$ROOT" && cargo test --workspace --locked -- --test-threads=1)
      fi
    fi
  fi
fi

if [[ $minimal -eq 0 ]]; then
  if [[ "${ARW_DOCGEN_SKIP_BUILDS:-0}" == 1 ]]; then
    info "Skipping docgen and packaging (ARW_DOCGEN_SKIP_BUILDS=1)."
  else
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
