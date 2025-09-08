#!/usr/bin/env bash
set -euo pipefail
command -v cargo >/dev/null || { echo 'cargo not found'; exit 1; }
echo "[test] Running cargo nextest (workspace)"
cargo nextest run --workspace
