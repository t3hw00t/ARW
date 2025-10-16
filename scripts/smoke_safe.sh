#!/usr/bin/env bash
set -euo pipefail

# Standard low-impact defaults for runtime smoke tests.
# Agents and humans can source this script or run `just smoke-safe` to apply them.

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Prefer release builds when available; require callers to build explicitly.
export ARW_SERVER_BIN="${ARW_SERVER_BIN:-${ROOT_DIR}/target/release/arw-server}"
export RUNTIME_SMOKE_USE_RELEASE="${RUNTIME_SMOKE_USE_RELEASE:-1}"
export RUNTIME_SMOKE_SKIP_BUILD="${RUNTIME_SMOKE_SKIP_BUILD:-1}"

# Keep processes polite (nice/ionice) and disable unsupported prompt-cache by default.
export RUNTIME_SMOKE_NICE="${RUNTIME_SMOKE_NICE:-1}"
export LLAMA_PROMPT_CACHE_PATH="${LLAMA_PROMPT_CACHE_PATH:-}"  # empty → skip auto flag

# Keep the worker pool tiny so stub runs do not spawn dozens of threads.
export ARW_WORKERS="${ARW_WORKERS:-4}"
export ARW_WORKERS_MAX="${ARW_WORKERS_MAX:-4}"

# Default to lightweight GPU options; callers can override as needed.
export LLAMA_GPU_LAYERS="${LLAMA_GPU_LAYERS:-4}"
export RUNTIME_SMOKE_GPU_POLICY="${RUNTIME_SMOKE_GPU_POLICY:-simulate}"

# Flag that safe defaults are in effect (suppresses reminders in the smoke suite).
export RUNTIME_SMOKE_SAFE_DEFAULTS=1

echo "[smoke-safe] Applied safe runtime smoke defaults." >&2
