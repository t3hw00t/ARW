#!/usr/bin/env bash
set -euo pipefail

# Orchestrates the managed runtime smoke tests.
# Always runs the baseline stub smoke unless RUNTIME_SMOKE_SKIP_STUB=1.
# GPU coverage is controlled via RUNTIME_SMOKE_GPU_POLICY:
#   skip|no|off   → do not run the GPU stage (default)
#   simulate|sim  → run the llama smoke in simulated GPU mode
#   auto|detect   → run with a real llama.cpp server when detected, otherwise simulate
#   require|must  → require a real llama.cpp server (fail if missing)
#
# Optional environment knobs:
#   RUNTIME_SMOKE_LLAMA_SERVER_BIN   → preferred llama-server path
#   RUNTIME_SMOKE_LLAMA_MODEL_PATH   → override LLAMA_MODEL_PATH for the GPU stage
#   RUNTIME_SMOKE_LLAMA_SERVER_ARGS  → extra args for llama-server
#   RUNTIME_SMOKE_LLAMA_SERVER_PORT  → fixed port for llama-server
#   RUNTIME_SMOKE_STUB_MODE          → stub backend mode (defaults to "stub")
#   LLAMA_GPU_LOG_PATTERN            → override GPU log detection regex

SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
SMOKE_SCRIPT="${PROJECT_ROOT}/scripts/runtime_llama_smoke.sh"
DEFAULT_LLAMA_SERVER="${PROJECT_ROOT}/cache/llama.cpp/build/bin/llama-server"

log() {
  printf '[runtime-smoke-suite] %s\n' "$*"
}

to_lower() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]'
}

is_truthy() {
  local value
  value="$(to_lower "${1:-}")"
  case "$value" in
    1|yes|true|on) return 0 ;;
    *) return 1 ;;
  esac
}

resolve_llama_server() {
  local candidate=""
  if [[ -n "${RUNTIME_SMOKE_LLAMA_SERVER_BIN:-}" ]]; then
    candidate="${RUNTIME_SMOKE_LLAMA_SERVER_BIN}"
    if [[ -x "$candidate" ]]; then
      export LLAMA_SERVER_BIN="$candidate"
      echo "$candidate"
      return 0
    fi
    log "Configured RUNTIME_SMOKE_LLAMA_SERVER_BIN is not executable: ${candidate}"
  fi

  if [[ -n "${LLAMA_SERVER_BIN:-}" ]]; then
    candidate="${LLAMA_SERVER_BIN}"
    if [[ -x "$candidate" ]]; then
      echo "$candidate"
      return 0
    fi
    log "LLAMA_SERVER_BIN is set but not executable: ${candidate}"
  fi

  if [[ -x "$DEFAULT_LLAMA_SERVER" ]]; then
    export LLAMA_SERVER_BIN="$DEFAULT_LLAMA_SERVER"
    echo "$DEFAULT_LLAMA_SERVER"
    return 0
  fi

  if command -v llama-server >/dev/null 2>&1; then
    candidate="$(command -v llama-server)"
    if [[ -n "$candidate" && -x "$candidate" ]]; then
      export LLAMA_SERVER_BIN="$candidate"
      echo "$candidate"
      return 0
    fi
  fi

  return 1
}

run_stub() {
  local stub_mode="${RUNTIME_SMOKE_STUB_MODE:-stub}"
  stub_mode="$(to_lower "$stub_mode")"
  log "Running managed runtime stub smoke (MODE=${stub_mode})"
  MODE="$stub_mode" bash "$SMOKE_SCRIPT"
}

run_gpu_simulated() {
  log "Running simulated GPU smoke (llama backend stub)"
  env MODE=gpu LLAMA_GPU_SIMULATE=1 bash "$SMOKE_SCRIPT"
}

run_gpu_real() {
  local server_bin="$1"
  local strict="${2:-0}"
  export LLAMA_SERVER_BIN="$server_bin"

  if [[ -n "${RUNTIME_SMOKE_LLAMA_MODEL_PATH:-}" ]]; then
    export LLAMA_MODEL_PATH="$RUNTIME_SMOKE_LLAMA_MODEL_PATH"
  fi
  if [[ -n "${RUNTIME_SMOKE_LLAMA_SERVER_ARGS:-}" ]]; then
    export LLAMA_SERVER_ARGS="$RUNTIME_SMOKE_LLAMA_SERVER_ARGS"
  fi
  if [[ -n "${RUNTIME_SMOKE_LLAMA_SERVER_PORT:-}" ]]; then
    export LLAMA_SERVER_PORT="$RUNTIME_SMOKE_LLAMA_SERVER_PORT"
  fi

  if [[ -z "${LLAMA_GPU_ENFORCE:-}" ]]; then
    export LLAMA_GPU_ENFORCE=1
  fi

  if [[ "$strict" == "1" ]]; then
    export LLAMA_GPU_REQUIRE_REAL=1
  else
    export LLAMA_GPU_REQUIRE_REAL=0
  fi

  log "Running real GPU smoke via ${server_bin}"
  if env MODE=gpu bash "$SMOKE_SCRIPT"; then
    return 0
  fi

  if [[ "$strict" == "1" ]]; then
    return 1
  fi

  log "Real GPU smoke failed; falling back to simulated GPU run"
  run_gpu_simulated
}

GPU_POLICY="${RUNTIME_SMOKE_GPU_POLICY:-skip}"
GPU_POLICY="$(to_lower "$GPU_POLICY")"

RUN_STUB=1
if is_truthy "${RUNTIME_SMOKE_SKIP_STUB:-0}"; then
  RUN_STUB=0
fi

if ! is_truthy "${RUNTIME_SMOKE_SAFE_DEFAULTS:-0}"; then
  log "Reminder: apply safe defaults via 'source scripts/smoke_safe.sh' or 'just runtime-smoke-safe' before running heavy smokes."
fi

if [[ $RUN_STUB -eq 1 ]]; then
  log "Plan: stub stage enabled"
  if [[ "${RUNTIME_SMOKE_DRY_RUN:-0}" = "1" ]]; then
    log "Dry-run requested; exiting before execution."
    exit 0
  fi
  run_stub
else
  log "Skipping stub smoke (RUNTIME_SMOKE_SKIP_STUB=1)"
fi

if [[ "${RUNTIME_SMOKE_DRY_RUN:-0}" = "1" ]]; then
  log "Dry-run requested; skipping GPU stage."
  exit 0
fi

case "$GPU_POLICY" in
  ""|skip|no|none|off)
    log "GPU smoke disabled (policy=${GPU_POLICY:-skip})"
    exit 0
    ;;
  simulate|sim|fake)
    run_gpu_simulated
    exit 0
    ;;
  auto|detect|maybe)
    if server_path="$(resolve_llama_server)"; then
      if run_gpu_real "$server_path" 0; then
        exit 0
      fi
      # run_gpu_real already falls back to simulated on failure.
      exit 0
    fi
    log "No llama-server binary detected; falling back to simulated GPU markers"
    run_gpu_simulated
    exit 0
    ;;
  require|must|force)
    if server_path="$(resolve_llama_server)"; then
      if run_gpu_real "$server_path" 1; then
        exit 0
      fi
      log "GPU policy 'require' enforced and real run failed"
      exit 1
    fi
    log "GPU policy 'require' set but no llama-server binary detected (set LLAMA_SERVER_BIN or RUNTIME_SMOKE_LLAMA_SERVER_BIN)"
    exit 1
    ;;
  *)
    log "Unknown RUNTIME_SMOKE_GPU_POLICY value: ${GPU_POLICY}"
    exit 1
    ;;
esac
