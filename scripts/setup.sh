#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"

yes_flag=0
no_docs=0
run_tests=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    -y|--yes) yes_flag=1; shift;;
    --no-docs) no_docs=1; shift;;
    --run-tests) run_tests=1; shift;;
    *) echo "Unknown option: $1"; exit 1;;
  esac
done

title(){ echo -e "\033[36m\n=== $* ===\033[0m"; }
info(){ echo -e "\033[36m[setup]\033[0m $*"; }
warn(){ echo -e "\033[33m[warn]\033[0m $*"; }
pause(){ [[ $yes_flag -eq 1 ]] || read -rp "$*" _; }

title "Prerequisites"
if ! command -v cargo >/dev/null 2>&1; then
  warn "Rust 'cargo' not found."
  echo "Install Rust via rustup: https://rustup.rs"
  pause "Press Enter after installing Rust (or Ctrl+C to abort)"
fi

mkdocs_ok=0
if command -v mkdocs >/dev/null 2>&1; then mkdocs_ok=1; fi
if [[ $no_docs -eq 0 && $mkdocs_ok -eq 0 ]]; then
  if command -v python3 >/dev/null 2>&1; then
    info "Installing MkDocs via pip"
    python3 -m pip install --upgrade pip || true
    python3 -m pip install mkdocs mkdocs-material mkdocs-git-revision-date-localized-plugin || true
    command -v mkdocs >/dev/null 2>&1 && mkdocs_ok=1 || warn "MkDocs install failed; docs site will be skipped"
  else
    warn "python3 not found; skipping docs site build"
  fi
fi

title "Build workspace (release)"
(cd "$ROOT" && cargo build --workspace --release --locked)

if [[ $run_tests -eq 1 ]]; then
  title "Run tests (workspace)"
  cargo nextest run --workspace --locked
fi

title "Generate workspace status page"
if command -v jq >/dev/null 2>&1; then
  bash "$DIR/docgen.sh" || warn "docgen failed"
else
  warn "jq not found; skipping docgen page generation (install: apt-get install jq | brew install jq)"
fi

if [[ $no_docs -eq 0 && $mkdocs_ok -eq 1 ]]; then
  title "Build docs site (MkDocs)"
  (cd "$ROOT" && mkdocs build --strict)
else
  info "Skipping docs site build"
fi

title "Package portable bundle"
bash "$DIR/package.sh" --no-build
info "Done. See dist/ for portable bundle."
