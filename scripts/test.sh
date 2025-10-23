#!/usr/bin/env bash
set -euo pipefail
command -v cargo >/dev/null || { echo 'cargo not found'; exit 1; }

if command -v cargo-nextest >/dev/null 2>&1; then
  echo "[test] Running cargo nextest (workspace)"
  cargo nextest run --workspace --locked --test-threads=1
else
  echo "[test] cargo-nextest not found; running cargo test instead."
  echo "[test] Install it with 'cargo install --locked cargo-nextest' for faster runs."
  cargo test --workspace --locked -- --test-threads=1
fi
