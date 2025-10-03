#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

run_cli() {
  if command -v arw-cli >/dev/null 2>&1; then
    exec arw-cli smoke context "$@"
  fi

  local exe="arw-cli"
  [[ "${OS:-}" == "Windows_NT" ]] && exe="arw-cli.exe"
  for candidate in \
    "$ROOT_DIR/target/release/$exe" \
    "$ROOT_DIR/target/debug/$exe"; do
    if [[ -x "$candidate" ]]; then
      exec "$candidate" smoke context "$@"
    fi
  done

  if command -v cargo >/dev/null 2>&1; then
    cd "$ROOT_DIR"
    exec cargo run --quiet --release -p arw-cli -- smoke context "$@"
  fi

  echo "error: unable to locate arw-cli binary; install it or run cargo build -p arw-cli" >&2
  exit 1
}

run_cli "$@"
