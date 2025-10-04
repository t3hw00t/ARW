#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
source "$SCRIPT_DIR/lib/smoke_timeout.sh"
smoke_timeout::init "runtime-smoke" 600 "RUNTIME_SMOKE_TIMEOUT_SECS"

# Runtime smoke test for the managed runtime pipeline.
# Default MODE=stub spins up a tiny Python HTTP server that mimics llama.cpp's `/completion`
# endpoint so CI can exercise `chat.respond` without model weights. Use MODE=real with
# LLAMA_SERVER_BIN/LLAMA_MODEL_PATH (and optional LLAMA_SERVER_ARGS/LLAMA_SERVER_PORT)
# to target an actual llama.cpp server binary instead.

MODE=${MODE:-stub}
TMP_DIR=$(mktemp -d -t arw-runtime-smoke.XXXX)
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
  rm -rf "$TMP_DIR"
  return "$status"
}
trap cleanup EXIT

pick_port() {
  python3 - <<'PY'
import socket
with socket.socket() as s:
    s.bind(('127.0.0.1', 0))
    print(s.getsockname()[1])
PY
}

start_stub() {
  local port_file="$TMP_DIR/stub-port"
  python3 - "$port_file" "$BACKEND_LOG" "$BACKEND_REQ" <<'PY' &
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
    echo "[runtime-smoke] failed to allocate stub port" >&2
    exit 1
  fi
}

start_real_backend() {
  : "${LLAMA_SERVER_BIN:?set LLAMA_SERVER_BIN when MODE=real}" >&2
  : "${LLAMA_MODEL_PATH:?set LLAMA_MODEL_PATH when MODE=real}" >&2
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
    if [[ -n "$prompt_cache_path" ]]; then
      local args_join=" ${extra[*]} "
      if [[ "$args_join" != *" --prompt-cache "* && "$args_join" != *"--prompt-cache="* ]]; then
        cmd+=(--prompt-cache "$prompt_cache_path")
        echo "[runtime-smoke] appended --prompt-cache ${prompt_cache_path} to LLAMA_SERVER_ARGS" >&2
      fi
    fi
  else
    cmd+=(-m "$LLAMA_MODEL_PATH" --host 127.0.0.1 --port "$BACKEND_PORT" --log-disable)
    if [[ -n "$prompt_cache_path" ]]; then
      cmd+=(--prompt-cache "$prompt_cache_path")
    fi
  fi
  echo "[runtime-smoke] launching llama backend: ${cmd[*]}" >&2
  (cd "$PROJECT_ROOT" && "${cmd[@]}" >"$BACKEND_LOG" 2>&1 &)
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
  case "$MODE" in
    stub) start_stub ;;
    real) start_real_backend ;;
    *)
      echo "[runtime-smoke] unknown MODE=$MODE" >&2
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
  export ARW_LLAMA_URL="http://127.0.0.1:${BACKEND_PORT}"
  export RUST_LOG=${RUST_LOG:-info}
  export ARW_EGRESS_PROXY_ENABLE=0

  local server_bin="${ARW_SERVER_BIN:-}"
  if [[ -z "$server_bin" ]]; then
    if [[ -x "$PROJECT_ROOT/target/debug/arw-server" ]]; then
      server_bin="$PROJECT_ROOT/target/debug/arw-server"
    else
      echo "[runtime-smoke] building arw-server binary" >&2
      (cd "$PROJECT_ROOT" && cargo build -p arw-server >/dev/null)
      server_bin="$PROJECT_ROOT/target/debug/arw-server"
    fi
  fi

  (cd "$PROJECT_ROOT" && "$server_bin" >"$SERVER_LOG" 2>&1 &)
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

  python3 - "$CHAT_LOG" "$MODE" <<'PY'
import json
import sys
path, mode = sys.argv[1], sys.argv[2]
with open(path, 'r', encoding='utf-8') as fh:
    data = json.load(fh)
backend = data.get('backend')
text = data.get('reply', {}).get('content', '')
if backend != 'llama':
    raise SystemExit(f"unexpected backend: {backend!r}")
if not text.strip():
    raise SystemExit("empty llama reply")
if mode == 'stub' and 'llama-stub' not in text:
    raise SystemExit(f"stub backend reply missing marker: {text!r}")
PY

  if [[ "$MODE" = "stub" ]]; then
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

start_backend
start_server
chat_probe

echo "[runtime-smoke] chat.respond path exercised via llama backend (mode=${MODE})"
echo "[runtime-smoke] logs: $SERVER_LOG"
