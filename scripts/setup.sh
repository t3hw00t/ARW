#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$DIR/.." && pwd)"

WARNINGS=()
yes_flag=0
run_tests=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    -y|--yes) yes_flag=1; shift;;
    --run-tests) run_tests=1; shift;;
    *) echo "Unknown option: $1"; exit 1;;
  esac
done

title(){ echo -e "\033[36m\n=== $* ===\033[0m"; }
info(){ echo -e "\033[36m[setup]\033[0m $*"; }
warn(){ WARNINGS+=("$*"); }
pause(){ [[ $yes_flag -eq 1 ]] || read -rp "$*" _; }

title "Prerequisites"
if ! command -v cargo >/dev/null 2>&1; then
  warn "Rust 'cargo' not found."
  echo "Install Rust via rustup: https://rustup.rs"
  pause "Press Enter after installing Rust (or Ctrl+C to abort)"
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

title "Package portable bundle"
bash "$DIR/package.sh" --no-build
if [[ ${#WARNINGS[@]} -gt 0 ]]; then
  title "Warnings"
  for w in "${WARNINGS[@]}"; do
    echo -e "\033[33m- $w\033[0m"
  done
fi
info "Done. See dist/ for portable bundle."
