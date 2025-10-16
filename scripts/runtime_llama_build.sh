#!/usr/bin/env bash
set -euo pipefail

# Utility that configures and builds the llama.cpp server binary used by the
# runtime smoke suite. On success the path to the resulting executable is
# printed to stdout so callers can capture it.

log() {
  printf '[runtime-llama-build] %s\n' "$*" >&2
}

usage() {
  cat <<'EOF' >&2
Usage: scripts/runtime_llama_build.sh [options]

Options:
  --config <name>      CMake build configuration (default: Release)
  --build-dir <path>   Custom build directory (default: cache/llama.cpp/build)
  --generator <name>   Explicit CMake generator (falls back to CMake default)
  --force              Drop cached build files before reconfiguring
  -h, --help           Show this help message

Environment overrides:
  LLAMA_BUILD_CONFIG       Alternative configuration name (Release/Debug/…)
  LLAMA_BUILD_DIR          Custom build directory (same as --build-dir)
  LLAMA_CMAKE_GENERATOR    Generator name (same as --generator)
  LLAMA_BUILD_PARALLEL     Parallel build jobs passed to CMake --parallel
EOF
  exit 1
}

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
LLAMA_DIR="${PROJECT_ROOT}/cache/llama.cpp"

if [[ ! -d "$LLAMA_DIR" ]]; then
  log "llama.cpp checkout missing at ${LLAMA_DIR}"
  log "Did you clone the repository with its submodules?"
  exit 1
fi

if ! command -v cmake >/dev/null 2>&1; then
  log "cmake not found in PATH; install CMake 3.14+ to build llama-server"
  exit 1
fi

CONFIG="${LLAMA_BUILD_CONFIG:-Release}"
BUILD_DIR="${LLAMA_BUILD_DIR:-${LLAMA_DIR}/build}"
GENERATOR="${LLAMA_CMAKE_GENERATOR:-}"
PARALLEL="${LLAMA_BUILD_PARALLEL:-}"
FORCE_RECONFIG=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --config)
      [[ $# -ge 2 ]] || usage
      CONFIG="$2"
      shift 2
      ;;
    --build-dir)
      [[ $# -ge 2 ]] || usage
      BUILD_DIR="$2"
      shift 2
      ;;
    --generator)
      [[ $# -ge 2 ]] || usage
      GENERATOR="$2"
      shift 2
      ;;
    --force)
      FORCE_RECONFIG=1
      shift
      ;;
    -h|--help)
      usage
      ;;
    *)
      log "Unknown argument: $1"
      usage
      ;;
  esac
done

# Normalise build directory
case "$BUILD_DIR" in
  /*|?:/*|\\\\*)
    # absolute path on POSIX (/...), Windows (C:/...), or UNC (\\...)
    ;;
  *)
    BUILD_DIR="${LLAMA_DIR}/$BUILD_DIR"
    ;;
esac

CONFIG_SANITIZED="${CONFIG,,}"
case "$CONFIG_SANITIZED" in
  release|debug|minsizerel|relwithdebinfo)
    # keep original casing for multi-config generators
    ;;
  *)
    # No validation beyond non-empty requirement
    if [[ -z "$CONFIG" ]]; then
      CONFIG="Release"
    fi
    ;;
esac

mkdir -p "$BUILD_DIR"

if [[ "$FORCE_RECONFIG" -eq 1 ]]; then
  log "Forcing reconfigure: clearing ${BUILD_DIR}/CMakeCache.txt"
  rm -f "${BUILD_DIR}/CMakeCache.txt"
  rm -rf "${BUILD_DIR}/CMakeFiles"
fi

configure() {
  local cmd=(cmake)
  if [[ -n "$GENERATOR" ]]; then
    cmd+=(-G "$GENERATOR")
  fi
  cmd+=(
    -S "$LLAMA_DIR"
    -B "$BUILD_DIR"
    "-DCMAKE_BUILD_TYPE=${CONFIG}"
    -DLLAMA_BUILD_SERVER=ON
    -DLLAMA_BUILD_EXAMPLES=OFF
    -DLLAMA_BUILD_TESTS=OFF
    -DLLAMA_BUILD_TOOLS=ON
    -DLLAMA_BUILD_COMMON=ON
    -DLLAMA_FATAL_WARNINGS=OFF
    -DGGML_CUDA=OFF
    -DGGML_HIP=OFF
    -DGGML_METAL=OFF
    -DGGML_VULKAN=OFF
    -DGGML_SYCL=OFF
    -DGGML_OPENCL=OFF
    -DGGML_RPC=OFF
  )
  log "Configuring llama.cpp (build dir: ${BUILD_DIR})"
  "${cmd[@]}"
}

detect_parallel() {
  if [[ -n "$PARALLEL" ]]; then
    printf '%s' "$PARALLEL"
    return
  fi
  if command -v nproc >/dev/null 2>&1; then
    nproc
    return
  fi
  if [[ "$(uname -s 2>/dev/null)" == "Darwin" ]]; then
    sysctl -n hw.ncpu 2>/dev/null || printf '1'
    return
  fi
  if [[ -n "${NUMBER_OF_PROCESSORS:-}" ]]; then
    printf '%s' "$NUMBER_OF_PROCESSORS"
    return
  fi
  printf '1'
}

configure

build_cmd=(
  cmake
  --build "$BUILD_DIR"
  --target llama-server
  --config "$CONFIG"
)

jobs="$(detect_parallel)"
if [[ -n "$jobs" ]]; then
  if [[ "$jobs" =~ ^[0-9]+$ ]] && [[ "$jobs" -gt 0 ]]; then
    build_cmd+=(--parallel "$jobs")
  fi
fi

log "Building llama-server (config=${CONFIG}, parallel=${jobs:-1})"
"${build_cmd[@]}"

find_binary() {
  local base="$1"
  local config="$2"
  local config_lower="${config,,}"
  local config_upper="${config^^}"
  local config_capitalised="${config_lower^}"
  local candidates=(
    "${base}/bin/llama-server"
    "${base}/bin/llama-server.exe"
    "${base}/bin/${config}/llama-server"
    "${base}/bin/${config}/llama-server.exe"
    "${base}/bin/${config_upper}/llama-server.exe"
    "${base}/bin/${config_capitalised}/llama-server.exe"
    "${base}/${config}/llama-server"
    "${base}/${config}/llama-server.exe"
    "${base}/${config_upper}/llama-server.exe"
    "${base}/${config_capitalised}/llama-server.exe"
  )
  local candidate
  for candidate in "${candidates[@]}"; do
    if [[ -n "$candidate" && -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

if ! binary_path="$(find_binary "$BUILD_DIR" "$CONFIG")"; then
  log "Build completed but llama-server binary was not found under ${BUILD_DIR}"
  log "Inspect the build logs above for errors."
  exit 1
fi

log "llama-server ready at ${binary_path}"
printf '%s\n' "$binary_path"
