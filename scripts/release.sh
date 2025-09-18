#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")"/.. && pwd)
cd "$ROOT"

version=${1:-v0.1.0-beta}

echo "[release] checking clippy/tests"
cargo clippy -p arw-protocol -p arw-events -p arw-core -p arw-macros -p arw-cli -p arw-otel -p arw-server -p arw-connector --all-targets -- -D warnings
cargo test --workspace --locked --exclude arw-tauri --exclude arw-launcher

echo "[release] regenerating specs/docs"
OPENAPI_GEN=1 OPENAPI_OUT=spec/openapi.yaml cargo run -q -p arw-server
bash scripts/docgen.sh

echo "[release] tagging ${version}"
git add -A
git commit -m "chore(release): ${version} prep" || true
git tag -a "${version}" -m "${version} stability baseline"
echo "[release] run: git push origin main --follow-tags"
