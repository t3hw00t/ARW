#!/usr/bin/env bash
set -euo pipefail

SKIP_SMOKE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --weights-only|--skip-smoke)
      SKIP_SMOKE=1
      shift
      ;;
    -h|--help)
      cat <<'EOF'
Usage: just runtime-check [--weights-only]

--weights-only    Download weights and show mirror info without running the smoke test.
EOF
      exit 0
      ;;
    *)
      echo "[runtime-check] Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

ROOT_DIR=$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
RUNTIME_WEIGHTS_SCRIPT="${ROOT_DIR}/scripts/runtime_weights.py"
RUNTIME_SMOKE_SCRIPT="${ROOT_DIR}/scripts/runtime_llama_smoke.sh"
DEFAULT_LLAMA_SERVER="${ROOT_DIR}/cache/llama.cpp/build/bin/llama-server"

load_mirrors() {
  local python_bin=""
  if command -v python3 >/dev/null 2>&1; then
    python_bin="python3"
  elif command -v python >/dev/null 2>&1; then
    python_bin="python"
  else
    return 1
  fi
  "${python_bin}" - <<'PY'
import json
from pathlib import Path
import re
path = Path("configs/runtime/model_sources.json")
try:
    data = json.loads(path.read_text(encoding="utf-8"))
except (FileNotFoundError, json.JSONDecodeError):
    mirrors = []
else:
    mirrors = data.get("mirrors", [])
url_re = re.compile(r"^https://")
checksum_re = re.compile(r"^sha256:[0-9a-fA-F]{64}$")
for entry in mirrors:
    name = entry.get("name", "")
    url = entry.get("url", "")
    notes = entry.get("notes", "")
    checksum = entry.get("checksum", "")
    if not url:
        continue
    if not url_re.match(url):
        continue
    if checksum and not checksum_re.match(checksum):
        continue
    print("|".join([name, url, notes, checksum]))
PY
}

step() {
  printf '\n[%s] %s\n' "runtime-check" "$1"
}

ensure_token() {
  if [[ -n "${HF_TOKEN:-}" ]]; then
    return
  fi
  if [[ -n "${HUGGINGFACEHUB_API_TOKEN:-}" ]]; then
    export HF_TOKEN="$HUGGINGFACEHUB_API_TOKEN"
    return
  fi
  cat <<'EOF'
[runtime-check] Hugging Face access token required (scope: Read).
You can create one at https://huggingface.co/settings/tokens.
Token input is hidden; nothing is stored on disk.
EOF
  read -rsp "Paste token: " HF_TOKEN_INPUT
  echo
  if [[ -z "$HF_TOKEN_INPUT" ]]; then
    echo "[runtime-check] Token not provided, aborting." >&2
    exit 1
  fi
  export HF_TOKEN="$HF_TOKEN_INPUT"
}

detect_llama_server() {
  if [[ -n "${LLAMA_SERVER_BIN:-}" ]]; then
    if [[ -x "$LLAMA_SERVER_BIN" ]]; then
      return
    fi
    echo "[runtime-check] LLAMA_SERVER_BIN is set but not executable: $LLAMA_SERVER_BIN" >&2
  fi

  if [[ -x "$DEFAULT_LLAMA_SERVER" ]]; then
    export LLAMA_SERVER_BIN="$DEFAULT_LLAMA_SERVER"
    return
  fi

  read -rp "Path to llama-server binary (leave empty to skip GPU run): " LLAMA_SERVER_INPUT
  if [[ -z "$LLAMA_SERVER_INPUT" ]]; then
    echo "[runtime-check] llama-server not provided; the smoke test will fall back to simulated GPU mode." >&2
    unset LLAMA_SERVER_BIN
    return
  fi
  if [[ ! -x "$LLAMA_SERVER_INPUT" ]]; then
    echo "[runtime-check] Provided path is not executable: $LLAMA_SERVER_INPUT" >&2
    exit 1
  fi
  export LLAMA_SERVER_BIN="$LLAMA_SERVER_INPUT"
}

step "Checking Hugging Face token"
ensure_token

step "Suggested mirrors"
MIRRORS_RAW="$(load_mirrors || true)"
if [[ -n "$MIRRORS_RAW" ]]; then
  while IFS='|' read -r mirror_name mirror_url mirror_notes mirror_checksum; do
    [[ -z "$mirror_url" ]] && continue
    note_suffix=""
    if [[ -n "$mirror_notes" ]]; then
      note_suffix=" — $mirror_notes"
    fi
    if [[ -n "$mirror_checksum" ]]; then
      note_suffix="${note_suffix} (checksum ${mirror_checksum})"
    fi
    if [[ -n "$mirror_name" ]]; then
      printf '  • %s: %s%s\n' "$mirror_name" "$mirror_url" "$note_suffix"
    else
      printf '  • %s%s\n' "$mirror_url" "$note_suffix"
    fi
  done <<< "$MIRRORS_RAW"
else
  echo "  • No mirror information configured (see configs/runtime/model_sources.json)."
fi

step "Downloading runtime weights (cache/models)"
HF_TOKEN="$HF_TOKEN" python3 "$RUNTIME_WEIGHTS_SCRIPT" --all

if [[ $SKIP_SMOKE -eq 1 ]]; then
  step "Skipping smoke test (--weights-only requested)"
  exit 0
fi

step "Locating llama-server binary"
detect_llama_server

if [[ -z "${LLAMA_SERVER_BIN:-}" ]]; then
  step "Running smoke test in simulated GPU mode"
  HL_ENV="MODE=gpu LLAMA_GPU_SIMULATE=1"
  env MODE=gpu LLAMA_GPU_SIMULATE=1 bash "$RUNTIME_SMOKE_SCRIPT"
  exit 0
fi

step "Running smoke test with real GPU backend"
env MODE=gpu LLAMA_GPU_REQUIRE_REAL=1 LLAMA_SERVER_BIN="$LLAMA_SERVER_BIN" bash "$RUNTIME_SMOKE_SCRIPT"
