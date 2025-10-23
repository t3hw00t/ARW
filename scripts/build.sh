#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/build.sh [--debug] [--no-tests] [--with-launcher|--headless]

Options:
  --debug         Build without --release (faster iterative debug profile)
  --no-tests      Skip workspace tests after building
  --with-launcher Opt in to building the Tauri launcher (requires platform deps)
  --headless      Force headless build (default; skips arw-launcher package)
EOF
}

mode=release
run_tests=1
include_launcher=0

if [[ "${ARW_BUILD_LAUNCHER:-}" =~ ^(1|true|yes)$ ]]; then
  include_launcher=1
fi

while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug) mode=debug; shift;;
    --no-tests) run_tests=0; shift;;
    --with-launcher) include_launcher=1; shift;;
    --headless) include_launcher=0; shift;;
    --help|-h) usage; exit 0;;
    *) echo "Unknown arg: $1"; echo; usage; exit 2;;
  esac
done

command -v cargo >/dev/null || { echo 'cargo not found'; exit 1; }

build_flavour="headless (skipping arw-launcher)"
if [[ $include_launcher -eq 1 ]]; then
  build_flavour="full (includes arw-launcher)"
fi

echo "[build] Building workspace ($mode, $build_flavour)"
cargo_args=(build --workspace)
if [[ $include_launcher -eq 0 ]]; then
  cargo_args+=(--exclude arw-launcher)
fi
if [[ "$mode" == release ]]; then
  cargo_args+=(--release)
fi
cargo "${cargo_args[@]}"

if [[ $run_tests -eq 1 ]]; then
  if command -v cargo-nextest >/dev/null 2>&1; then
    echo "[build] Running tests (nextest)"
    cargo nextest run --workspace --locked --test-threads=1
  else
    echo "[build] cargo-nextest not found; falling back to cargo test."
    echo "[build] Install it with 'cargo install --locked cargo-nextest' for faster runs."
    cargo test --workspace --locked -- --test-threads=1
  fi
fi

echo "[build] Done."
