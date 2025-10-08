#!/usr/bin/env bash
set -euo pipefail

mode=release
run_tests=1
while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug) mode=debug; shift;;
    --no-tests) run_tests=0; shift;;
    *) echo "Unknown arg: $1"; exit 2;;
  esac
done

command -v cargo >/dev/null || { echo 'cargo not found'; exit 1; }

echo "[build] Building workspace ($mode)"
if [[ "$mode" == release ]]; then
  cargo build --workspace --release
else
  cargo build --workspace
fi

if [[ $run_tests -eq 1 ]]; then
  if command -v cargo-nextest >/dev/null 2>&1; then
    echo "[build] Running tests (nextest)"
    cargo nextest run --workspace --locked
  else
    echo "[build] cargo-nextest not found; falling back to cargo test."
    echo "[build] Install it with 'cargo install --locked cargo-nextest' for faster runs."
    cargo test --workspace --locked
  fi
fi

echo "[build] Done."
