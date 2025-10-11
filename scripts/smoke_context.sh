#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT_DIR=$(cd "${SCRIPT_DIR}/.." && pwd)
source "$SCRIPT_DIR/lib/smoke_timeout.sh"
smoke_timeout::init "smoke-context" 600 "SMOKE_CONTEXT_TIMEOUT_SECS"

cleanup() {
  local status=$?
  status=$(smoke_timeout::cleanup "$status")
  return "$status"
}
trap cleanup EXIT

run_command() {
  "$@" &
  local child=$!
  smoke_timeout::register_child "$child"
  set +e
  wait "$child"
  local status=$?
  set -e
  smoke_timeout::unregister_child "$child"
  return "$status"
}

run_cli() {
  if command -v arw-cli >/dev/null 2>&1; then
    run_command arw-cli smoke context "$@"
    return
  fi

  local exe="arw-cli"
  [[ "${OS:-}" == "Windows_NT" ]] && exe="arw-cli.exe"
  for candidate in \
    "$ROOT_DIR/target/release/$exe" \
    "$ROOT_DIR/target/debug/$exe"; do
    if [[ -x "$candidate" ]]; then
      run_command "$candidate" smoke context "$@"
      return
    fi
  done

  if command -v cargo >/dev/null 2>&1; then
    local prev_dir=$PWD
    cd "$ROOT_DIR"
    local cargo_args=()
    if [[ "${ARW_SMOKE_USE_RELEASE:-0}" == "1" ]]; then
      cargo_args=(run --quiet --release -p arw-cli -- smoke context)
    else
      cargo_args=(run --quiet -p arw-cli -- smoke context)
    fi
    cargo_args+=("$@")
    run_command cargo "${cargo_args[@]}"
    local status=$?
    cd "$prev_dir"
    return "$status"
  fi

  echo "error: unable to locate arw-cli binary; install it or run cargo build -p arw-cli" >&2
  exit 1
}

run_cli "$@"
