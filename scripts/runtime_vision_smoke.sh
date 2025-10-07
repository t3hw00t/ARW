#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
source "$SCRIPT_DIR/lib/smoke_timeout.sh"

RUNTIME_ID="${VISION_RUNTIME_ID:-vision.llava.preview}"
SMOKE_ROOT="${VISION_SMOKE_ROOT:-$PROJECT_ROOT/.smoke/vision}"
mkdir -p "$SMOKE_ROOT"
TMP_DIR=$(mktemp -d "$SMOKE_ROOT/run.XXXXXX")
SERVER_LOG="$TMP_DIR/arw-server.log"
MATRIX_JSON="$TMP_DIR/runtime-matrix.json"
STUB_SCRIPT="$TMP_DIR/vision_stub.py"
MANIFEST_PATH="$TMP_DIR/runtimes.toml"
STUB_LOG="$TMP_DIR/vision-stub.log"
CHAT_LOG="$TMP_DIR/vision-describe.json"
SERVER_PID=""

SMOKE_STATE_DIR="$TMP_DIR/state"
SMOKE_DATA_DIR="$TMP_DIR/data"
SMOKE_CACHE_DIR="$TMP_DIR/cache"
mkdir -p "$SMOKE_STATE_DIR" "$SMOKE_DATA_DIR" "$SMOKE_CACHE_DIR"
TMP_SUBDIR="$TMP_DIR/tmp"
mkdir -p "$TMP_SUBDIR"
export TMPDIR="$TMP_SUBDIR"

cleanup() {
  local status=$?
  status=$(smoke_timeout::cleanup "$status")
  if [[ -n "$SERVER_PID" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  if [[ -n "${STUB_SCRIPT:-}" ]]; then
    pkill -f "$STUB_SCRIPT" 2>/dev/null || true
  fi
  if [[ $status -ne 0 ]]; then
    echo "[vision-smoke] server log (tail)" >&2
    tail -n 200 "$SERVER_LOG" >&2 || true
    if [[ -s "$STUB_LOG" ]]; then
      echo "[vision-smoke] stub log (tail)" >&2
      tail -n 200 "$STUB_LOG" >&2 || true
    fi
  fi
  rm -rf "$TMP_DIR"
  return "$status"
}
trap cleanup EXIT

if [[ "${ARW_SMOKE_USE_SYNTHETIC:-0}" = "1" ]]; then
  echo "[vision-smoke] ARW_SMOKE_USE_SYNTHETIC=1 — skipping (no local sockets)." >&2
  exit 0
fi

pick_port() {
  python3 -c 'import socket;
s = socket.socket();
s.bind(("127.0.0.1", 0));
p = s.getsockname()[1];
s.close();
print(p)' || {
    echo "[vision-smoke] failed to allocate port" >&2
    exit 1
  }
}

VISION_PORT=$(pick_port)

cat >"$STUB_SCRIPT" <<'PY'
#!/usr/bin/env python3
import json
import os
import signal
import sys
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PORT = int(os.environ.get("VISION_STUB_PORT", "12801"))
LOG_PATH = os.environ.get("VISION_STUB_LOG")

def log(msg: str) -> None:
    if LOG_PATH:
        try:
            with open(LOG_PATH, "a", encoding="utf-8") as fh:
                fh.write(msg + "\n")
        except OSError:
            pass

log(f"START:{PORT}")

class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        log("HTTP " + fmt % args)

    def _write(self, status: HTTPStatus, body: bytes, content_type: str = "application/json"):
        self.send_response(status.value)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if self.path == "/healthz":
            self._write(HTTPStatus.OK, b"ready", "text/plain")
        else:
            self._write(HTTPStatus.NOT_FOUND, b"{}")

    def do_POST(self):
        length = int(self.headers.get("content-length") or 0)
        raw = self.rfile.read(length) if length else b"{}"
        try:
            payload = json.loads(raw.decode("utf-8"))
        except json.JSONDecodeError:
            payload = {}
        log(f"POST:{self.path}:{json.dumps(payload)[:120]}")
        if self.path == "/describe":
            reply = {
                "ok": True,
                "summary": "vision-stub:description",
                "inputs": payload,
            }
            body = json.dumps(reply).encode("utf-8")
            self._write(HTTPStatus.OK, body)
        else:
            self._write(HTTPStatus.NOT_FOUND, b"{}")

    def do_PUT(self):
        self.do_POST()

def handle_shutdown(signum, frame):
    raise SystemExit(0)

signal.signal(signal.SIGTERM, handle_shutdown)
signal.signal(signal.SIGINT, handle_shutdown)

server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
try:
    server.serve_forever(poll_interval=0.5)
except SystemExit:
    log("STOP")
finally:
    server.server_close()
PY
chmod +x "$STUB_SCRIPT"

cat >"$MANIFEST_PATH" <<EOF
version = 1

[[runtimes]]
id = "${RUNTIME_ID}"
adapter = "process"
name = "Vision Stub Smoke"
profile = "describe"
modalities = ["vision"]
accelerator = "gpu_cuda"
auto_start = true
preset = "balanced"
tags = { bundle = "vision.smoke.stub" }

[runtimes.process]
command = "${STUB_SCRIPT}"
args = []

[runtimes.process.env]
VISION_STUB_PORT = "${VISION_PORT}"
VISION_STUB_LOG = "${STUB_LOG}"

[runtimes.process.health]
url = "http://127.0.0.1:${VISION_PORT}/healthz"
method = "GET"
expect_status = 200
expect_body = "ready"
timeout_ms = 2000
EOF

smoke_timeout::init "vision-smoke" 480 "VISION_SMOKE_TIMEOUT_SECS"

start_server() {
  export ARW_STATE_DIR="$SMOKE_STATE_DIR"
  export ARW_DATA_DIR="$SMOKE_DATA_DIR"
  export ARW_CACHE_DIR="$SMOKE_CACHE_DIR"
export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-vision-smoke-token}"
export ARW_BIND="127.0.0.1"
export ARW_PORT="${ARW_PORT:-$(pick_port)}"
export ARW_RUNTIME_SUPERVISOR=1
export ARW_RUNTIME_MANIFEST="$MANIFEST_PATH"
export ARW_SMOKE_MODE="${ARW_SMOKE_MODE:-vision}"
export ARW_EGRESS_PROXY_ENABLE=0
export RUST_LOG=${RUST_LOG:-info}
export ARW_KERNEL_ENABLE=0
export ARW_SQLITE_POOL_MIN="${ARW_SQLITE_POOL_MIN:-1}"
export ARW_SQLITE_POOL_MAX="${ARW_SQLITE_POOL_MAX:-4}"
export ARW_SQLITE_POOL_SIZE="${ARW_SQLITE_POOL_SIZE:-2}"
export ARW_SQLITE_POOL_AUTOTUNE=0
export ARW_SQLITE_CHECKPOINT_SEC="${ARW_SQLITE_CHECKPOINT_SEC:-0}"
export ARW_RUNTIME_RESTART_MAX="${ARW_RUNTIME_RESTART_MAX:-2}"
export ARW_RUNTIME_RESTART_WINDOW_SEC="${ARW_RUNTIME_RESTART_WINDOW_SEC:-120}"
export ARW_RUNTIME_MATRIX_TTL_SEC="${ARW_RUNTIME_MATRIX_TTL_SEC:-20}"

  local server_bin="${ARW_SERVER_BIN:-}"
  if [[ -z "$server_bin" ]]; then
    echo "[vision-smoke] ARW_SERVER_BIN must point to an existing arw-server binary; refusing to auto-build to avoid resource spikes." >&2
    exit 1
  fi

  "$server_bin" >"$SERVER_LOG" 2>&1 &
  SERVER_PID=$!

  local healthz="http://127.0.0.1:${ARW_PORT}/healthz"
  for _ in {1..240}; do
    if curl -fsS "$healthz" >/dev/null 2>&1; then
      echo "[vision-smoke] arw-server ready on port ${ARW_PORT}"
      return 0
    fi
    sleep 0.5
  done
  echo "[vision-smoke] server did not become healthy in time" >&2
  exit 1
}

fetch_runtime_matrix() {
  local base="http://127.0.0.1:${ARW_PORT}"
  curl -fsS "$base/state/runtime_matrix" \
    -H "X-ARW-Admin: ${ARW_ADMIN_TOKEN}" \
    -H "Authorization: Bearer ${ARW_ADMIN_TOKEN}" \
    -H "Accept: application/json" \
    -o "$MATRIX_JSON"
}

wait_for_runtime_state() {
  local expected="$1"
  local attempts="${2:-180}"
  for ((i = 0; i < attempts; i++)); do
    if fetch_runtime_matrix && python3 - "$MATRIX_JSON" "$RUNTIME_ID" "$expected" <<'PY'
import json
import sys

path, runtime_id, expected = sys.argv[1:4]
with open(path, "r", encoding="utf-8") as fh:
    data = json.load(fh)
items = data.get("items", {})
entry = items.get(runtime_id)
if not isinstance(entry, dict):
    raise SystemExit(1)
status = entry.get("status") or {}
code = status.get("code")
if not isinstance(code, str):
    raise SystemExit(1)
if code.lower() != expected.lower():
    raise SystemExit(1)
detail = status.get("detail") or []
if not detail:
    raise SystemExit("runtime status missing detail entries")
print(f"{runtime_id} -> {code}")
PY
    then
      return 0
    fi
    sleep 1
  done
  echo "[vision-smoke] runtime ${RUNTIME_ID} failed to reach state ${expected}" >&2
  return 1
}

wait_for_stub_starts() {
  local expected="$1"
  for _ in {1..120}; do
    if [[ -s "$STUB_LOG" ]] && python3 - "$STUB_LOG" "$expected" <<'PY'
import sys
from pathlib import Path

path = Path(sys.argv[1])
expected = int(sys.argv[2])

text = path.read_text(encoding="utf-8")
starts = [line for line in text.splitlines() if line.startswith("START:")]
if len(starts) >= expected:
    sys.exit(0)
sys.exit(1)
PY
    then
      return 0
    fi
    sleep 0.5
  done
  echo "[vision-smoke] stub did not report ${expected} start(s)" >&2
  return 1
}

describe_probe() {
  local payload='{"image": "stub.jpg"}'
  if ! curl -fsS -X POST "http://127.0.0.1:${VISION_PORT}/describe" \
    -H "Content-Type: application/json" \
    -d "$payload" >"$CHAT_LOG"; then
    echo "[vision-smoke] failed to call stub describe endpoint" >&2
    return 1
  fi
  python3 - "$CHAT_LOG" <<'PY'
import json
import sys
path = sys.argv[1]
with open(path, "r", encoding="utf-8") as fh:
    data = json.load(fh)
if not data.get("ok"):
    raise SystemExit("vision stub response missing ok flag")
summary = data.get("summary", "")
if "vision-stub" not in summary:
    raise SystemExit(f"unexpected summary: {summary!r}")
PY
}

request_restore() {
  local base="http://127.0.0.1:${ARW_PORT}"
  curl -fsS -X POST "$base/orchestrator/runtimes/${RUNTIME_ID}/restore" \
    -H "X-ARW-Admin: ${ARW_ADMIN_TOKEN}" \
    -H "Authorization: Bearer ${ARW_ADMIN_TOKEN}" \
    -H "Content-Type: application/json" \
    -d '{"restart": true}' >/dev/null
}

start_server

wait_for_runtime_state "ready"
wait_for_stub_starts 1

describe_probe

request_restore
wait_for_runtime_state "ready"
wait_for_stub_starts 2

echo "[vision-smoke] runtime ${RUNTIME_ID} restored successfully"
