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
is_linux=0

case "$(uname -s 2>/dev/null)" in
  Linux|Linux-*) is_linux=1 ;;
esac

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
if [[ $include_launcher -eq 0 || $is_linux -eq 1 ]]; then
  cargo_args+=(--exclude arw-launcher)
fi
if [[ "$mode" == release ]]; then
  cargo_args+=(--release)
fi
cargo "${cargo_args[@]}"

if [[ $include_launcher -eq 1 && $is_linux -eq 1 ]]; then
  launcher_args=(build -p arw-launcher)
  if [[ "$mode" == release ]]; then
    launcher_args+=(--release)
  fi
  launcher_args+=(--features launcher-linux-ui)
  echo "[build] Building arw-launcher (${mode})"
  cargo "${launcher_args[@]}"
fi

if [[ $run_tests -eq 1 ]]; then
  if command -v cargo-nextest >/dev/null 2>&1; then
    echo "[build] Running tests (nextest)"
    nextest_args=(run --workspace --locked --test-threads=1)
    if [[ $include_launcher -eq 0 || $is_linux -eq 1 ]]; then
      nextest_args+=(--exclude arw-launcher)
    fi
    cargo nextest "${nextest_args[@]}"
    if [[ $include_launcher -eq 1 && $is_linux -eq 1 ]]; then
      launcher_nextest_args=(run -p arw-launcher --locked --test-threads=1)
      launcher_nextest_args+=(--features launcher-linux-ui)
      cargo nextest "${launcher_nextest_args[@]}"
    fi
  else
    echo "[build] cargo-nextest not found; falling back to cargo test."
    echo "[build] Install it with 'cargo install --locked cargo-nextest' for faster runs."
    test_args=(--workspace --locked)
    if [[ $include_launcher -eq 0 || $is_linux -eq 1 ]]; then
      test_args+=(--exclude arw-launcher)
    fi
    cargo test "${test_args[@]}" -- --test-threads=1
    if [[ $include_launcher -eq 1 && $is_linux -eq 1 ]]; then
      launcher_test_args=(-p arw-launcher --features launcher-linux-ui -- --test-threads=1)
      cargo test "${launcher_test_args[@]}"
    fi
  fi
fi

echo "[build] Done."
