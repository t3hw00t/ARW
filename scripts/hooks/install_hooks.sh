#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")"/../.. && pwd)
cd "$ROOT"

if [[ ! -d .git ]]; then
  echo "[hooks] Not a git repository: $ROOT" >&2
  exit 1
fi

mkdir -p .git/hooks
cat > .git/hooks/pre-commit << 'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "[pre-commit] cargo fmt --check"
cargo fmt --all -- --check
echo "[pre-commit] cargo clippy -D warnings"
cargo clippy --workspace --all-targets -- -D warnings
if command -v cargo-nextest >/dev/null 2>&1; then
  echo "[pre-commit] cargo nextest run"
  cargo nextest run --workspace
else
  echo "[pre-commit] cargo test"
  cargo test --workspace --locked
fi
EOF
chmod +x .git/hooks/pre-commit
echo "[hooks] Installed .git/hooks/pre-commit"

