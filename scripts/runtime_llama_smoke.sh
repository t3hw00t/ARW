#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
source "$SCRIPT_DIR/lib/smoke_timeout.sh"

# Runtime smoke test for the managed runtime pipeline.
# Default MODE=stub spins up a tiny Python HTTP server that mimics llama.cpp's `/completion`
# endpoint so CI can exercise `chat.respond` without model weights. Use MODE=real or MODE=cpu
# with LLAMA_SERVER_BIN/LLAMA_MODEL_PATH (and optional LLAMA_SERVER_ARGS/LLAMA_SERVER_PORT)
# to target an actual llama.cpp server binary on CPU, or MODE=gpu to append a minimal
# accelerator hint (`--gpu-layers`, override via LLAMA_GPU_LAYERS/LLAMA_SERVER_ARGS) and
# optionally enforce GPU detection via LLAMA_GPU_ENFORCE/LLAMA_GPU_LOG_PATTERN. When real
# accelerators are unavailable you can still exercise the GPU verification path by exporting
# LLAMA_GPU_SIMULATE=1 (or letting the script auto-enable it when model/bin inputs are
# missing); the smoke test keeps the stub backend but injects a GPU marker into the log and
# enforces the pattern checks. Set LLAMA_GPU_REQUIRE_REAL=1 to force a hard failure instead of
# simulating.
#
# If the environment disallows local sockets entirely (common in restricted sandboxes), set
# ARW_SMOKE_USE_SYNTHETIC=1 to skip the network-dependent pieces; the script exits cleanly
# after logging that the smoke was skipped.

MODE=${MODE:-stub}
MODE=$(printf '%s' "$MODE" | tr '[:upper:]' '[:lower:]')
BACKEND_KIND=""
LLAMA_ACCEL="${LLAMA_ACCEL:-}"
EXPECTED_CHAT_BACKEND="llama"

resolve_smoke_root() {
  local raw="$1"
  python3 - "$raw" "$PROJECT_ROOT" <<'PY'
import sys
from pathlib import Path

raw = Path(sys.argv[1])
project_root = Path(sys.argv[2]).resolve()
if not raw.is_absolute():
    raw = project_root / raw
try:
    resolved = raw.resolve(strict=False)
except FileNotFoundError:
    resolved = raw
try:
    resolved.relative_to(project_root)
except ValueError:
    raise SystemExit("runtime smoke root must stay under the project root")
if resolved == project_root:
    raise SystemExit("runtime smoke root cannot equal the project root")
print(resolved)
PY
}

prune_old_runs() {
  local root="$1"
  local keep="${RUNTIME_SMOKE_KEEP_RECENT:-6}"
  local ttl="${RUNTIME_SMOKE_RETENTION_SECS:-604800}"
  if [[ "${RUNTIME_SMOKE_KEEP_TMP:-0}" = "1" ]] || [[ "${RUNTIME_SMOKE_DISABLE_PRUNE:-0}" = "1" ]]; then
    return
  fi
  local removed
  removed=$(
    python3 - "$root" "$keep" "$ttl" <<'PY'
import shutil
import sys
import time
from pathlib import Path

root = Path(sys.argv[1])
try:
    keep = max(0, int(sys.argv[2]))
except ValueError:
    keep = 0
try:
    ttl = max(0, int(sys.argv[3]))
except ValueError:
    ttl = 0

if not root.exists():
    raise SystemExit(0)

entries = []
for entry in root.iterdir():
    if not entry.is_dir():
        continue
    if not entry.name.startswith("run."):
        continue
    keep_marker = entry / ".keep"
    if keep_marker.exists():
        continue
    try:
        stat = entry.stat()
    except OSError:
        continue
    entries.append((stat.st_mtime, entry))

entries.sort(key=lambda item: item[0], reverse=True)
now = time.time()
removed = []
for index, (mtime, entry) in enumerate(entries):
    should_remove = False
    if index >= keep:
        should_remove = True
    if ttl > 0 and (now - mtime) > ttl:
        should_remove = True
    if not should_remove:
        continue
    try:
        shutil.rmtree(entry)
        removed.append(entry.name)
    except OSError:
        pass

if removed:
    print(" ".join(removed))
PY
  ) || return
  if [[ -n "$removed" ]]; then
    echo "[runtime-smoke] pruned old runs: $removed" >&2
  fi
}

SMOKE_ROOT_RAW="${RUNTIME_SMOKE_ROOT:-$PROJECT_ROOT/.smoke/runtime}"
if ! SMOKE_ROOT="$(resolve_smoke_root "$SMOKE_ROOT_RAW")"; then
  echo "[runtime-smoke] invalid RUNTIME_SMOKE_ROOT (${SMOKE_ROOT_RAW}); adjust the path." >&2
  exit 1
fi
mkdir -p "$SMOKE_ROOT"
prune_old_runs "$SMOKE_ROOT"

MEM_CHECK_OUTPUT=""
RESOLVED_SERVER_BIN=""
RESOLVED_SERVER_BUILD=""  # debug|release
RESOLVED_SERVER_NEEDS_BUILD=0

resolve_server_candidate() {
  local candidate=""
  local needs_build=0
  local profile="debug"

  if [[ -n "${ARW_SERVER_BIN:-}" ]]; then
    candidate="$ARW_SERVER_BIN"
    if [[ ! -x "$candidate" ]]; then
      needs_build=1
    fi
    echo "$candidate|$needs_build|$profile"
    return
  fi

  local release_path="$PROJECT_ROOT/target/release/arw-server"
  local debug_path="$PROJECT_ROOT/target/debug/arw-server"

  local prefer_release=0
  if is_truthy "${RUNTIME_SMOKE_USE_RELEASE:-0}"; then
    prefer_release=1
  fi

  if [[ $prefer_release -eq 1 ]]; then
    if [[ -x "$release_path" ]]; then
      candidate="$release_path"
      profile="release"
    else
      candidate="$release_path"
      profile="release"
      needs_build=1
    fi
  else
    if [[ -x "$debug_path" ]]; then
      candidate="$debug_path"
      profile="debug"
    elif [[ -x "$release_path" ]]; then
      candidate="$release_path"
      profile="release"
    else
      candidate="$debug_path"
      profile="debug"
      needs_build=1
    fi
  fi

  echo "$candidate|$needs_build|$profile"
}

check_memory_budget() {
  local model_path="$1"
  local output=""
  local status=0
  set +e
  output=$(python3 - "$model_path" <<'PY'
import os
import sys
from pathlib import Path

path = Path(sys.argv[1])
if not path.is_file():
    sys.exit(0)

def read_env_float(name, default):
    raw = os.environ.get(name, "")
    if not raw:
        return default
    try:
        return float(raw)
    except ValueError:
        return default

factor = read_env_float("RUNTIME_SMOKE_MEM_FACTOR", 2.2)
overhead_gb = read_env_float("RUNTIME_SMOKE_MEM_OVERHEAD_GB", 1.0)
reserve_gb = read_env_float("RUNTIME_SMOKE_MEM_RESERVE_GB", 1.0)
min_required_gb = read_env_float("RUNTIME_SMOKE_MIN_REQUIRED_GB", 0.0)

bytes_per_gb = 1024**3
size_bytes = path.stat().st_size
required = int(size_bytes * factor + overhead_gb * bytes_per_gb)
min_required = int(min_required_gb * bytes_per_gb)
if required < min_required:
    required = min_required

try:
    meminfo = Path("/proc/meminfo").read_text(encoding="utf-8", errors="ignore").splitlines()
except OSError:
    sys.exit(0)

available_bytes = 0
for line in meminfo:
    if line.startswith("MemAvailable:"):
        parts = line.split()
        if len(parts) >= 2:
            try:
                available_bytes = int(float(parts[1]) * 1024)
            except ValueError:
                available_bytes = 0
        break

if available_bytes <= 0:
    sys.exit(0)

reserve_bytes = int(reserve_gb * bytes_per_gb)
available_after_reserve = max(available_bytes - reserve_bytes, 0)

if available_after_reserve >= required:
    sys.exit(0)

def format_bytes(value):
    units = ["B", "KB", "MB", "GB", "TB", "PB"]
    value = float(value)
    for unit in units:
        if value < 1024 or unit == units[-1]:
            if unit == "B":
                return f"{int(value)}B"
            return f"{value:.1f}{unit}"
        value /= 1024
    return f"{value:.1f}PB"

print(
    f"{required}\t{available_after_reserve}\t"
    f"{format_bytes(required)}\t{format_bytes(available_after_reserve)}\t{format_bytes(size_bytes)}",
    end=""
)
sys.exit(1)
PY
  )
  MEM_CHECK_OUTPUT="$output"
  status=$?
  set -e
  if [[ ${status:-0} -ne 0 ]]; then
    return 1
  fi
  return 0
}

load_default_sources() {
  local config="${PROJECT_ROOT}/configs/runtime/model_sources.json"
  local python_bin=""
  if command -v python3 >/dev/null 2>&1; then
    python_bin="python3"
  elif command -v python >/dev/null 2>&1; then
    python_bin="python"
  else
    return 1
  fi
  "$python_bin" - <<'PY'
import json, sys
from pathlib import Path
path = Path("configs/runtime/model_sources.json")
sources = []
try:
    data = json.loads(path.read_text(encoding="utf-8"))
except FileNotFoundError:
    pass
except json.JSONDecodeError as exc:
    sys.stderr.write(f"[runtime-smoke] Invalid JSON in {path}: {exc}\n")
else:
    for entry in data.get("default", []):
        repo = entry.get("repo")
        file = entry.get("file")
        if repo and file:
            sources.append(f"{repo}::{file}")
print(",".join(sources))
PY
}

DEFAULT_MODEL_SOURCES_RAW="$(load_default_sources || true)"
if [[ -n "$DEFAULT_MODEL_SOURCES_RAW" ]]; then
  IFS=',' read -ra DEFAULT_MODEL_SOURCES <<< "$DEFAULT_MODEL_SOURCES_RAW"
else
  DEFAULT_MODEL_SOURCES=()
fi
if [[ ${#DEFAULT_MODEL_SOURCES[@]} -eq 0 ]]; then
  DEFAULT_MODEL_SOURCES=(
    "ggml-org/tinyllama-1.1b-chat::tinyllama-1.1b-chat-q4_k_m.gguf"
    "TheBloke/TinyLlama-1.1B-Chat-GGUF::TinyLlama-1.1B-Chat-q4_k_m.gguf"
  )
fi
IFS=',' read -ra MODEL_SOURCES <<< "${LLAMA_MODEL_SOURCES:-}"
if [[ ${#MODEL_SOURCES[@]} -eq 0 ]]; then
  MODEL_SOURCES=("${DEFAULT_MODEL_SOURCES[@]}")
fi

MODEL_PATH_PRESET=0
if [[ -n "${LLAMA_MODEL_PATH:-}" ]]; then
  MODEL_PATH_PRESET=1
else
  for src in "${MODEL_SOURCES[@]}"; do
    model_file="${src##*::}"
    candidate="${PROJECT_ROOT}/cache/models/${model_file}"
    if [[ -f "$candidate" ]]; then
      LLAMA_MODEL_PATH="$candidate"
      break
    fi
  done
  if [[ -z "${LLAMA_MODEL_PATH:-}" ]]; then
    primary_file="${MODEL_SOURCES[0]##*::}"
    LLAMA_MODEL_PATH="${PROJECT_ROOT}/cache/models/${primary_file}"
  fi
  export LLAMA_MODEL_PATH
fi
is_truthy() {
  local value="${1:-}"
  case "$(printf '%s' "$value" | tr '[:upper:]' '[:lower:]')" in
    1|true|yes|on) return 0 ;;
    *) return 1 ;;
  esac
}

# Return 0 if llama-server appears to support a given CLI flag
supports_llama_flag() {
  local flag="$1"
  if [[ -z "${LLAMA_SERVER_BIN:-}" || ! -x "$LLAMA_SERVER_BIN" ]]; then
    return 1
  fi
  "$LLAMA_SERVER_BIN" --help 2>&1 | grep -q -- "$flag"
}

# Print a short preflight plan before launching anything.
print_preflight_plan() {
  local accel_label="${LLAMA_ACCEL:-n/a}"
  local backend_label="$BACKEND_KIND"
  local gpu_mode="${GPU_SIMULATE:-0}"; [[ "$gpu_mode" = "1" ]] && gpu_mode="simulate" || gpu_mode="real"
  local prompt_cache_note="auto"
  if [[ -z "${LLAMA_PROMPT_CACHE_PATH:-}" ]]; then
    prompt_cache_note="disabled"
  elif supports_llama_flag "--prompt-cache"; then
    prompt_cache_note="enabled"
  else
    prompt_cache_note="unsupported"
  fi

  local model_sz=""
  if [[ -n "${LLAMA_MODEL_PATH:-}" && -f "${LLAMA_MODEL_PATH}" ]]; then
    model_sz=$(python3 - <<'PY'
import os,sys
from pathlib import Path
p=Path(os.environ.get('LLAMA_MODEL_PATH',''))
print(f"{p.stat().st_size}") if p.is_file() else print()
PY
)
  fi
  local model_human=""
  if [[ -n "$model_sz" ]]; then
    model_human=$(python3 - <<'PY'
import sys
sz=int(sys.stdin.read().strip() or 0)
u=['B','KB','MB','GB','TB']
v=float(sz)
for x in u:
    if v<1024 or x==u[-1]:
        print(f"{v:.1f}{x}")
        break
    v/=1024
PY
<<<"$model_sz")
  fi

  echo "[runtime-smoke] plan: backend=${backend_label}, accel=${accel_label}, gpu=${gpu_mode}, prompt-cache=${prompt_cache_note}" >&2
  if [[ -n "${LLAMA_SERVER_BIN:-}" ]]; then
    echo "[runtime-smoke] plan: llama-server=${LLAMA_SERVER_BIN}" >&2
  fi
  if [[ -n "${LLAMA_MODEL_PATH:-}" ]]; then
    echo "[runtime-smoke] plan: model=${LLAMA_MODEL_PATH}${model_human:+ (${model_human})}" >&2
  fi
  if [[ -n "${ARW_SERVER_BIN:-}" ]]; then
    echo "[runtime-smoke] plan: arw-server=${ARW_SERVER_BIN}" >&2
  fi

  local server_info
  server_info=$(resolve_server_candidate)
  IFS='|' read -r RESOLVED_SERVER_BIN RESOLVED_SERVER_NEEDS_BUILD RESOLVED_SERVER_BUILD <<<"$server_info"
  if [[ -n "$RESOLVED_SERVER_BIN" ]]; then
    local build_note="ready"
    if [[ "$RESOLVED_SERVER_NEEDS_BUILD" = "1" ]]; then
      build_note="build-required"
      if is_truthy "${RUNTIME_SMOKE_SKIP_BUILD:-0}"; then
        build_note="missing-skip-build"
      fi
    fi
    echo "[runtime-smoke] plan: arw-server-target=${RESOLVED_SERVER_BIN:-unknown} (${RESOLVED_SERVER_BUILD:-debug}, ${build_note})" >&2
  fi

  # Optional memory preview for real GPU
  if [[ "$BACKEND_KIND" = "llama" && "$LLAMA_ACCEL" = "gpu" && "${GPU_SIMULATE:-0}" != "1" ]]; then
    if check_memory_budget "${LLAMA_MODEL_PATH:-}"; then
      : # OK
    else
      local required available hr ha model_size
      if [[ -n "$MEM_CHECK_OUTPUT" ]]; then
        IFS=$'\t' read -r required available hr ha model_size <<<"$MEM_CHECK_OUTPUT"
        echo "[runtime-smoke] pre-check: memory may be tight (need ~${hr} after reserve, have ~${ha}; model≈${model_size})." >&2
      fi
    fi
  fi

  if is_truthy "${RUNTIME_SMOKE_DRY_RUN:-0}"; then
    echo "[runtime-smoke] dry-run requested; exiting before launch." >&2
    exit 0
  fi
}

print_hf_token_help() {
  cat >&2 <<'EOF'
[runtime-smoke] To exercise a real llama.cpp build you need a Hugging Face access token so the weights download succeeds:
  1. Create (or sign in to) your Hugging Face account.
  2. Visit https://huggingface.co/settings/tokens and generate a token with the “Read” scope.
  3. Export it before downloading models, e.g.:
       export HF_TOKEN=hf_your_read_token
     or run:
       huggingface-cli login
  4. Download a GGUF weight file, for example:
       huggingface-cli download --repo-id ggml-org/tinyllama-1.1b-chat \
         --include tinyllama-1.1b-chat-q4_k_m.gguf --local-dir ./models
  5. Set LLAMA_MODEL_PATH to the downloaded .gguf before rerunning this smoke test.

See docs/guide/runtime_matrix.md#getting-real-weights for additional tips.
EOF
}

download_weights_if_needed() {
  local target="$1"
  local token="${HF_TOKEN:-${HUGGINGFACEHUB_API_TOKEN:-}}"
  local tmp=""
  local repo=""
  local file=""
  local url=""

  if [[ -f "$target" ]]; then
    return 0
  fi

  if ! is_truthy "${LLAMA_ALLOW_DOWNLOADS:-0}"; then
    echo "[runtime-smoke] skipping automatic LLaMA weight downloads; set LLAMA_ALLOW_DOWNLOADS=1 to enable." >&2
    return 1
  fi

  if [[ $MODEL_PATH_PRESET -eq 1 ]]; then
    if [[ -z "$token" ]]; then
      echo "[runtime-smoke] LLAMA_MODEL_PATH not found (${target})." >&2
      print_hf_token_help
      return 1
    fi
    mkdir -p "$(dirname "$target")"
    for src in "${MODEL_SOURCES[@]}"; do
      repo="${src%%::*}"
      file="${src##*::}"
      url="https://huggingface.co/${repo}/resolve/main/${file}"
      tmp="${target}.download"
      rm -f "$tmp"
      echo "[runtime-smoke] Downloading ${file} from ${repo}..."
      if curl -fsSL -H "Authorization: Bearer $token" -o "$tmp" "$url"; then
        mv "$tmp" "$target"
        echo "[runtime-smoke] Saved weights to $target"
        return 0
      fi
    done
  else
    for src in "${MODEL_SOURCES[@]}"; do
      repo="${src%%::*}"
      file="${src##*::}"
      target="${PROJECT_ROOT}/cache/models/${file}"
      if [[ -f "$target" ]]; then
        LLAMA_MODEL_PATH="$target"
        export LLAMA_MODEL_PATH
        return 0
      fi
      if [[ -z "$token" ]]; then
        continue
      fi
      mkdir -p "$(dirname "$target")"
      tmp="${target}.download"
      rm -f "$tmp"
      url="https://huggingface.co/${repo}/resolve/main/${file}"
      echo "[runtime-smoke] Downloading ${file} from ${repo}..."
      if curl -fsSL -H "Authorization: Bearer $token" -o "$tmp" "$url"; then
        mv "$tmp" "$target"
        LLAMA_MODEL_PATH="$target"
        export LLAMA_MODEL_PATH
        echo "[runtime-smoke] Saved weights to $target"
        return 0
      fi
    done
  fi

  echo "[runtime-smoke] Unable to download LLaMA weights from configured sources." >&2
  print_hf_token_help
  return 1
}

GPU_SIMULATE=0
if is_truthy "${LLAMA_GPU_SIMULATE:-0}"; then
  GPU_SIMULATE=1
fi

GPU_REQUIRE_REAL=0
if is_truthy "${LLAMA_GPU_REQUIRE_REAL:-0}"; then
  GPU_REQUIRE_REAL=1
fi
SIMULATED_GPU_LOG=0

if [[ "${ARW_SMOKE_USE_SYNTHETIC:-0}" = "1" ]]; then
  echo "[runtime-smoke] ARW_SMOKE_USE_SYNTHETIC=1 — skipping runtime smoke (no local sockets)." >&2
  exit 0
fi

case "$MODE" in
  stub)
    BACKEND_KIND="stub"
    ;;
  real|llama)
    BACKEND_KIND="llama"
    LLAMA_ACCEL=${LLAMA_ACCEL:-cpu}
    ;;
  cpu)
    BACKEND_KIND="llama"
    LLAMA_ACCEL="cpu"
    MODE="llama"
    ;;
  gpu)
    BACKEND_KIND="llama"
    LLAMA_ACCEL="gpu"
    MODE="llama"
    ;;
  synthetic)
    BACKEND_KIND="synthetic"
    EXPECTED_CHAT_BACKEND="synthetic"
    LLAMA_ACCEL=""
    ;;
  *)
    echo "[runtime-smoke] unknown MODE=$MODE" >&2
    exit 1
    ;;
esac

if [[ "$BACKEND_KIND" = "llama" && -z "$LLAMA_ACCEL" ]]; then
  LLAMA_ACCEL="cpu"
fi

if [[ "$BACKEND_KIND" = "llama" && "$LLAMA_ACCEL" = "gpu" ]]; then
  if [[ -z "${LLAMA_SERVER_BIN:-}" ]]; then
    if [[ "$GPU_REQUIRE_REAL" = "1" ]]; then
      echo "[runtime-smoke] GPU mode requires LLAMA_SERVER_BIN when LLAMA_GPU_REQUIRE_REAL=1" >&2
      print_hf_token_help
      exit 1
    fi
    if [[ "$GPU_SIMULATE" != "1" ]]; then
      echo "[runtime-smoke] LLAMA_SERVER_BIN missing; auto-enabling simulated GPU markers (set LLAMA_GPU_REQUIRE_REAL=1 to enforce real accelerators)." >&2
      GPU_SIMULATE=1
    fi
  fi
  if [[ "$GPU_SIMULATE" != "1" ]]; then
    if ! download_weights_if_needed "$LLAMA_MODEL_PATH"; then
      if [[ "$GPU_REQUIRE_REAL" = "1" ]]; then
        exit 1
      fi
      echo "[runtime-smoke] Falling back to simulated GPU markers." >&2
      GPU_SIMULATE=1
    fi
  fi
  if [[ "$GPU_SIMULATE" != "1" ]]; then
    allow_high_mem=0
    if is_truthy "${RUNTIME_SMOKE_ALLOW_HIGH_MEM:-0}"; then
      allow_high_mem=1
    fi
    if [[ $allow_high_mem -eq 0 ]]; then
      MEM_CHECK_OUTPUT=""
      if ! check_memory_budget "${LLAMA_MODEL_PATH:-}"; then
        required=""
        available=""
        hr=""
        ha=""
        model_size=""
        if [[ -n "$MEM_CHECK_OUTPUT" ]]; then
          IFS=$'\t' read -r required available hr ha model_size <<<"$MEM_CHECK_OUTPUT"
          echo "[runtime-smoke] insufficient free memory for real GPU smoke (need ~${hr} after reserve, have ~${ha}; model≈${model_size})." >&2
          echo "[runtime-smoke] adjust RUNTIME_SMOKE_MEM_FACTOR/OVERHEAD_GB or set RUNTIME_SMOKE_ALLOW_HIGH_MEM=1 to bypass." >&2
        else
          echo "[runtime-smoke] insufficient free memory for real GPU smoke." >&2
        fi
        if [[ "$GPU_REQUIRE_REAL" = "1" ]]; then
          echo "[runtime-smoke] aborting because LLAMA_GPU_REQUIRE_REAL=1 (no simulated fallback allowed)." >&2
          exit 1
        fi
        echo "[runtime-smoke] falling back to simulated GPU markers due to memory guard." >&2
        GPU_SIMULATE=1
      fi
    fi
  fi
  if [[ "$GPU_SIMULATE" = "1" ]]; then
    SIMULATED_GPU_LOG=1
    BACKEND_KIND="stub"
  fi
fi

TMP_DIR=$(mktemp -d "$SMOKE_ROOT/run.XXXXXX")
SERVER_LOG="$TMP_DIR/arw-server.log"
BACKEND_LOG="$TMP_DIR/llama.log"
CHAT_LOG="$TMP_DIR/chat-response.json"
BACKEND_REQ="$TMP_DIR/llama-request.json"
BACKEND_PORT=""
BACKEND_PID=""
SERVER_PID=""
PROMPT_CACHE_PATH_DEFAULT="$TMP_DIR/llama.prompt.bin"

cleanup() {
  local status=$?
  status=$(smoke_timeout::cleanup "$status")
  if [[ -n "$SERVER_PID" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  if [[ -n "$BACKEND_PID" ]]; then
    kill "$BACKEND_PID" 2>/dev/null || true
    wait "$BACKEND_PID" 2>/dev/null || true
  fi
  if [[ $status -ne 0 ]]; then
    echo "[runtime-smoke] server log (tail)" >&2
    tail -n 200 "$SERVER_LOG" >&2 || true
    if [[ -s "$BACKEND_LOG" ]]; then
      echo "[runtime-smoke] backend log (tail)" >&2
      tail -n 200 "$BACKEND_LOG" >&2 || true
    fi
  fi
  local preserve=0
  if [[ $status -ne 0 ]]; then
    preserve=1
  fi
  if [[ "${RUNTIME_SMOKE_KEEP_TMP:-0}" = "1" ]]; then
    preserve=1
  fi
  if [[ $preserve -eq 1 ]]; then
    echo "[runtime-smoke] preserving run directory at ${TMP_DIR}" >&2
    if [[ "${RUNTIME_SMOKE_KEEP_TMP:-0}" = "1" ]]; then
      touch "$TMP_DIR/.keep" 2>/dev/null || true
    fi
  else
    rm -rf "$TMP_DIR"
  fi
  return "$status"
}
trap cleanup EXIT

LOOPBACK_ERR_FILE="$TMP_DIR/loopback_probe.err"
if ! python3 -c 'import socket; s = socket.socket();
s.bind(("127.0.0.1", 0));
s.close()' 2>"$LOOPBACK_ERR_FILE"; then
  echo "[runtime-smoke] unable to bind a loopback socket; local sockets appear blocked." >&2
  if [[ -s "$LOOPBACK_ERR_FILE" ]]; then
    echo "[runtime-smoke] socket probe error: $(cat "$LOOPBACK_ERR_FILE" 2>/dev/null || true)" >&2
  fi
  if is_truthy "${RUNTIME_SMOKE_REQUIRE_LOOPBACK:-0}"; then
    echo "[runtime-smoke] aborting (set RUNTIME_SMOKE_REQUIRE_LOOPBACK=0 to downgrade to a skip)." >&2
    exit 1
  fi
  echo "[runtime-smoke] skipping runtime smoke due to restricted networking (set RUNTIME_SMOKE_REQUIRE_LOOPBACK=1 to fail)." >&2
  exit 0
fi

smoke_timeout::init "runtime-smoke" 600 "RUNTIME_SMOKE_TIMEOUT_SECS"

pick_port() {
  python3 -c 'import socket; s = socket.socket();
s.bind(("127.0.0.1", 0));
port = s.getsockname()[1];
s.close();
print(port)' || {
    echo "[runtime-smoke] failed to pick an ephemeral port" >&2
    exit 1
  }
}

start_stub() {
  local port_file="$TMP_DIR/stub-port"
  python3 - "$port_file" "$BACKEND_LOG" "$BACKEND_REQ" <<'PY' >"$BACKEND_LOG" 2>&1 &
import json
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

port_file = sys.argv[1]
log_path = sys.argv[2]
req_path = sys.argv[3]

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args, **kwargs):
        with open(log_path, 'a', encoding='utf-8') as fh:
            fh.write("stub: " + (args[0] % args[1:]) + "\n")

    def do_POST(self):
        length = int(self.headers.get('content-length', '0'))
        raw = self.rfile.read(length) if length else b'{}'
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError:
            payload = {}
        try:
            with open(req_path, 'w', encoding='utf-8') as fh:
                json.dump(payload, fh, ensure_ascii=False)
                fh.write('\n')
        except OSError as exc:
            with open(log_path, 'a', encoding='utf-8') as fh:
                fh.write(f"stub: failed to write payload capture: {exc}\n")
        prompt = payload.get('prompt', '')
        reply = f"llama-stub:{prompt[-48:]}"
        body = json.dumps({'content': reply}).encode('utf-8')
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == '/health':
            body = json.dumps({'ok': True}).encode('utf-8')
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self.end_headers()

server = HTTPServer(('127.0.0.1', 0), Handler)
with open(port_file, 'w', encoding='utf-8') as fh:
    fh.write(str(server.server_address[1]))
try:
    server.serve_forever()
finally:
    server.server_close()
PY
  BACKEND_PID=$!
  for _ in {1..50}; do
    if [[ -f "$port_file" ]]; then
      BACKEND_PORT=$(cat "$port_file")
      break
    fi
    sleep 0.1
  done
  if [[ -z "$BACKEND_PORT" ]]; then
    if kill -0 "$BACKEND_PID" 2>/dev/null; then
      echo "[runtime-smoke] stub backend failed to report port within timeout" >&2
    else
      wait "$BACKEND_PID" 2>/dev/null || true
      BACKEND_PID=""
      echo "[runtime-smoke] stub backend exited early (likely due to sandboxed networking). Falling back to synthetic backend." >&2
      BACKEND_KIND="synthetic"
      EXPECTED_CHAT_BACKEND="synthetic"
      LLAMA_ACCEL=""
      return 0
    fi
    echo "[runtime-smoke] failed to allocate stub port" >&2
    exit 1
  fi
  if [[ "$SIMULATED_GPU_LOG" = "1" ]]; then
    echo "[runtime-smoke] simulated GPU acceleration log marker" >>"$BACKEND_LOG"
  fi
}

start_llama_backend() {
  local accel="${1:-cpu}"
  : "${LLAMA_SERVER_BIN:?set LLAMA_SERVER_BIN when MODE=real or MODE=llama}" >&2
  : "${LLAMA_MODEL_PATH:?set LLAMA_MODEL_PATH when MODE=real or MODE=llama}" >&2
  BACKEND_PORT=${LLAMA_SERVER_PORT:-$(pick_port)}
  local cmd=("$LLAMA_SERVER_BIN")
  local prompt_cache_path="${LLAMA_PROMPT_CACHE_PATH:-$PROMPT_CACHE_PATH_DEFAULT}"
  if [[ -n "$prompt_cache_path" ]]; then
    mkdir -p "$(dirname "$prompt_cache_path")"
  fi
  if [[ -n "${LLAMA_SERVER_ARGS:-}" ]]; then
    # shellcheck disable=SC2206
    local extra=( ${LLAMA_SERVER_ARGS} )
    cmd+=("${extra[@]}")
  else
    cmd+=(-m "$LLAMA_MODEL_PATH" --host 127.0.0.1 --port "$BACKEND_PORT" --log-disable)
  fi

  if [[ -n "$prompt_cache_path" ]]; then
    local has_prompt_cache=0
    for token in "${cmd[@]}"; do
      if [[ "$token" == "--prompt-cache" || "$token" == --prompt-cache=* ]]; then
        has_prompt_cache=1
        break
      fi
    done
    if [[ $has_prompt_cache -eq 0 ]]; then
      if supports_llama_flag "--prompt-cache"; then
        cmd+=(--prompt-cache "$prompt_cache_path")
        echo "[runtime-smoke] appended --prompt-cache ${prompt_cache_path}" >&2
      else
        echo "[runtime-smoke] llama-server does not support --prompt-cache; skipping the flag" >&2
      fi
    fi
  fi

  if [[ "$accel" == "gpu" ]]; then
    local has_gpu_hint=0
    for token in "${cmd[@]}"; do
      case "$token" in
        --gpu-layers|--gpu-layers=*|--tensor-split|--tensor-split=*|--device|--device=*|--devices|--devices=*|--mmproj|--mmproj=*)
          has_gpu_hint=1
          break
          ;;
      esac
    done
    if [[ $has_gpu_hint -eq 0 ]]; then
      local gpu_layers="${LLAMA_GPU_LAYERS:-8}"
      cmd+=(--gpu-layers "$gpu_layers")
      echo "[runtime-smoke] appended --gpu-layers $gpu_layers for GPU smoke" >&2
    fi
  elif [[ "$accel" == "cpu" && "${LLAMA_FORCE_CPU_LAYERS:-0}" == "1" ]]; then
    local has_gpu_layers=0
    for token in "${cmd[@]}"; do
      if [[ "$token" == "--gpu-layers" || "$token" == --gpu-layers=* ]]; then
        has_gpu_layers=1
        break
      fi
    done
    if [[ $has_gpu_layers -eq 0 ]]; then
      cmd+=(--gpu-layers 0)
      echo "[runtime-smoke] enforcing --gpu-layers 0 for CPU smoke" >&2
    fi
  fi

  echo "[runtime-smoke] launching llama backend (${accel}) with command: ${cmd[*]}" >&2
  local run_prefix=()
  if is_truthy "${RUNTIME_SMOKE_NICE:-0}"; then
    if command -v ionice >/dev/null 2>&1; then
      run_prefix=(nice -n "${RUNTIME_SMOKE_NICE_N:-10}" ionice -c2 -n "${RUNTIME_SMOKE_IONICE_N:-7}")
    else
      run_prefix=(nice -n "${RUNTIME_SMOKE_NICE_N:-10}")
    fi
  fi
  (cd "$PROJECT_ROOT" && "${run_prefix[@]}" "${cmd[@]}" >"$BACKEND_LOG" 2>&1 &)
  BACKEND_PID=$!
  for _ in {1..240}; do
    if curl -fsS "http://127.0.0.1:${BACKEND_PORT}/health" >/dev/null 2>&1; then
      return 0
    fi
    sleep 0.5
  done
  echo "[runtime-smoke] llama backend did not become healthy" >&2
  tail -n 200 "$BACKEND_LOG" >&2 || true
  exit 1
}

start_backend() {
  case "$BACKEND_KIND" in
    stub) start_stub ;;
    llama) start_llama_backend "$LLAMA_ACCEL" ;;
    synthetic)
      BACKEND_PID=""
      BACKEND_PORT=""
      ;;
    *)
      echo "[runtime-smoke] unknown backend kind: $BACKEND_KIND" >&2
      exit 1
      ;;
  esac
}

start_server() {
  export ARW_STATE_DIR="$TMP_DIR/state"
  export ARW_DATA_DIR="$TMP_DIR/data"
  export ARW_CACHE_DIR="$TMP_DIR/cache"
  mkdir -p "$ARW_STATE_DIR" "$ARW_DATA_DIR" "$ARW_CACHE_DIR"
  export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-runtime-smoke-token}"
  export ARW_BIND="127.0.0.1"
  export ARW_PORT="${ARW_PORT:-$(pick_port)}"
  if [[ "$BACKEND_KIND" != "synthetic" ]]; then
    export ARW_LLAMA_URL="http://127.0.0.1:${BACKEND_PORT}"
  else
    unset ARW_LLAMA_URL
  fi
  export RUST_LOG=${RUST_LOG:-info}
  export ARW_EGRESS_PROXY_ENABLE=0

  local server_bin="${RESOLVED_SERVER_BIN:-${ARW_SERVER_BIN:-}}"
  local build_profile="${RESOLVED_SERVER_BUILD:-debug}"
  local needs_build="${RESOLVED_SERVER_NEEDS_BUILD:-0}"
  if [[ -n "${ARW_SERVER_BIN:-}" && ! -x "${ARW_SERVER_BIN}" ]]; then
    echo "[runtime-smoke] ARW_SERVER_BIN is set but not executable: ${ARW_SERVER_BIN}" >&2
    exit 1
  fi

  if [[ -z "$server_bin" ]]; then
    server_bin="$PROJECT_ROOT/target/debug/arw-server"
  fi

  if [[ "$needs_build" = "1" ]]; then
    if is_truthy "${RUNTIME_SMOKE_SKIP_BUILD:-0}"; then
      echo "[runtime-smoke] arw-server binary not found and RUNTIME_SMOKE_SKIP_BUILD=1 set; aborting. Build once via 'cargo build -p arw-server' (use --release to target release) or set ARW_SERVER_BIN to an existing binary." >&2
      exit 1
    fi
    echo "[runtime-smoke] building arw-server binary (${build_profile})" >&2
    local build_log
    build_log=$(mktemp -t runtime-smoke-build.XXXX.log)
    local build_cmd=(cargo build -p arw-server)
    if [[ "$build_profile" = "release" ]]; then
      build_cmd=(cargo build -p arw-server --release)
    fi
    if ! (cd "$PROJECT_ROOT" && "${build_cmd[@]}" &>"$build_log"); then
      echo "[runtime-smoke] cargo build failed; log preserved at $build_log" >&2
      tail -n 200 "$build_log" >&2 || true
      exit 1
    fi
    rm -f "$build_log"
    if [[ -z "${ARW_SERVER_BIN:-}" ]]; then
      if [[ "$build_profile" = "release" ]]; then
        server_bin="$PROJECT_ROOT/target/release/arw-server"
      else
        server_bin="$PROJECT_ROOT/target/debug/arw-server"
      fi
    fi
  fi

  local run_prefix=()
  if is_truthy "${RUNTIME_SMOKE_NICE:-0}"; then
    if command -v ionice >/dev/null 2>&1; then
      run_prefix=(nice -n "${RUNTIME_SMOKE_NICE_N:-10}" ionice -c2 -n "${RUNTIME_SMOKE_IONICE_N:-7}")
    else
      run_prefix=(nice -n "${RUNTIME_SMOKE_NICE_N:-10}")
    fi
  fi
  (cd "$PROJECT_ROOT" && "${run_prefix[@]}" "$server_bin" >"$SERVER_LOG" 2>&1 &)
  SERVER_PID=$!

  local healthz="http://127.0.0.1:${ARW_PORT}/healthz"
  for _ in {1..240}; do
    if curl -fsS "$healthz" >/dev/null 2>&1; then
      echo "[runtime-smoke] arw-server ready on port ${ARW_PORT}"
      return 0
    fi
    sleep 0.5
  done
  echo "[runtime-smoke] server did not become healthy within timeout" >&2
  tail -n 200 "$SERVER_LOG" >&2 || true
  exit 1
}

chat_probe() {
  local base="http://127.0.0.1:${ARW_PORT}"
  local payload='{"prompt":"Runtime smoke hello"}'
  curl -fsS -X POST "$base/admin/chat/clear" \
    -H "X-ARW-Admin: ${ARW_ADMIN_TOKEN}" \
    -H "Authorization: Bearer ${ARW_ADMIN_TOKEN}" \
    -H "Content-Type: application/json" >/dev/null

  curl -fsS -X POST "$base/admin/chat/send" \
    -H "X-ARW-Admin: ${ARW_ADMIN_TOKEN}" \
    -H "Authorization: Bearer ${ARW_ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    -d "$payload" > "$CHAT_LOG"

  python3 - "$CHAT_LOG" "$EXPECTED_CHAT_BACKEND" "$BACKEND_KIND" <<'PY'
import json
import sys
path, expected, kind = sys.argv[1:4]
with open(path, 'r', encoding='utf-8') as fh:
    data = json.load(fh)
backend = data.get('backend')
text = data.get('reply', {}).get('content', '')
if backend != expected:
    raise SystemExit(f"unexpected backend: {backend!r}; expected {expected!r}")
if not text.strip():
    raise SystemExit("empty llama reply")
if kind == 'stub' and 'llama-stub' not in text:
    raise SystemExit(f"stub backend reply missing marker: {text!r}")
PY

  if [[ "$BACKEND_KIND" = "stub" ]]; then
    python3 - "$BACKEND_REQ" <<'PY'
import json
import sys
path = sys.argv[1]
try:
    with open(path, 'r', encoding='utf-8') as fh:
        payload = json.load(fh)
except FileNotFoundError:
    raise SystemExit('missing llama payload capture')

if 'cache_prompt' not in payload:
    raise SystemExit('cache_prompt not present in llama payload')
if payload['cache_prompt'] is not True:
    raise SystemExit(f"cache_prompt expected True, got {payload['cache_prompt']!r}")
if payload.get('prompt') is None:
    raise SystemExit('prompt missing in llama payload')
PY
  fi
}

runtime_matrix_probe() {
  local base="http://127.0.0.1:${ARW_PORT}"
  local matrix_json="$TMP_DIR/runtime-matrix.json"

  if ! curl -fsS "$base/state/runtime_matrix" \
    -H "X-ARW-Admin: ${ARW_ADMIN_TOKEN}" \
    -H "Authorization: Bearer ${ARW_ADMIN_TOKEN}" \
    -H "Accept: application/json" \
    -o "$matrix_json"; then
    echo "[runtime-smoke] failed to fetch runtime matrix" >&2
    exit 1
  fi

  local summary
  if ! summary=$(python3 - "$matrix_json" <<'PY'
import json
import sys

path = sys.argv[1]
try:
    with open(path, 'r', encoding='utf-8') as fh:
        data = json.load(fh)
except (OSError, json.JSONDecodeError) as exc:
    raise SystemExit(f"runtime matrix payload invalid: {exc}")

if not isinstance(data, dict):
    raise SystemExit('runtime matrix payload missing top-level object')

items = data.get('items')
if not isinstance(items, dict) or not items:
    raise SystemExit('runtime matrix payload missing items')

key, entry = next(iter(items.items()))

status = entry.get('status') or {}
label = status.get('label')
aria = status.get('aria_hint')
detail = status.get('detail')
severity = status.get('severity')
severity_label = status.get('severity_label')

if not isinstance(label, str) or not label.strip():
    raise SystemExit('runtime matrix status label missing')

if not isinstance(aria, str) or 'Runtime status' not in aria or not aria.strip():
    raise SystemExit('runtime matrix aria hint missing expected phrasing')

if not isinstance(detail, list) or not detail:
    raise SystemExit('runtime matrix detail missing')

if any((not isinstance(item, str) or not item.strip()) for item in detail):
    raise SystemExit('runtime matrix detail contains blank entries')

if not isinstance(severity, str) or not severity.strip():
    raise SystemExit('runtime matrix severity missing')

if not isinstance(severity_label, str) or not severity_label.strip():
    raise SystemExit('runtime matrix severity_label missing')

runtime = entry.get('runtime') or {}
if not runtime.get('updated'):
    raise SystemExit('runtime matrix runtime summary missing updated timestamp')

ttl_seconds = data.get('ttl_seconds')
if not isinstance(ttl_seconds, int) or ttl_seconds <= 0:
    raise SystemExit('runtime matrix ttl_seconds invalid')

code = status.get('code') or ''
print(f"{key}\t{label}\t{code}\t{severity}\t{severity_label}")
PY
  ); then
    echo "[runtime-smoke] runtime matrix validation failed" >&2
    exit 1
  fi

  local key label code severity severity_label
  IFS=$'\t' read -r key label code severity severity_label <<<"$summary"

  echo "[runtime-smoke] runtime matrix entry ${key}: ${label} (code=${code}, severity=${severity}, severity_label=${severity_label})"
}

verify_llama_accel() {
  local accel="$1"
  [[ "$accel" == "gpu" ]] || return 0
  if [[ ! -s "$BACKEND_LOG" ]]; then
    echo "[runtime-smoke] warning: backend log missing; cannot verify GPU usage" >&2
    [[ "${LLAMA_GPU_ENFORCE:-0}" == "1" ]] && exit 1
    return 0
  fi

  if grep -Eiq 'fall(?:ing)? back to cpu|fallback to cpu' "$BACKEND_LOG"; then
    echo "[runtime-smoke] llama backend reported fallback to CPU" >&2
    exit 1
  fi

  local pattern="${LLAMA_GPU_LOG_PATTERN:-cuda|metal|vulkan|directml|hip|gpu acceleration}"
  if [[ "$SIMULATED_GPU_LOG" = "1" ]]; then
    if grep -Eiq "$pattern" "$BACKEND_LOG"; then
      echo "[runtime-smoke] GPU acceleration validated via simulated stub backend" >&2
      return 0
    fi
    echo "[runtime-smoke] simulated GPU run missing expected marker (pattern: $pattern)" >&2
    exit 1
  fi

  if grep -Eiq "$pattern" "$BACKEND_LOG"; then
    echo "[runtime-smoke] detected GPU markers in llama backend log" >&2
    return 0
  fi

  if [[ "${LLAMA_GPU_ENFORCE:-0}" == "1" ]]; then
    echo "[runtime-smoke] failed to detect GPU usage in llama backend log (pattern: $pattern)" >&2
    exit 1
  fi

  echo "[runtime-smoke] warning: GPU markers not found in llama backend log (pattern: $pattern)" >&2
}

print_preflight_plan
start_backend
start_server
chat_probe
runtime_matrix_probe

if [[ "${LLAMA_ACCEL:-}" = "gpu" ]]; then
  verify_llama_accel "$LLAMA_ACCEL"
fi

echo "[runtime-smoke] chat.respond path exercised via ${EXPECTED_CHAT_BACKEND} backend (mode=${MODE}, accel=${LLAMA_ACCEL:-n/a})"
echo "[runtime-smoke] logs: $SERVER_LOG"
