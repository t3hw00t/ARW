#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PROJECT_ROOT=$(cd -- "$SCRIPT_DIR/.." && pwd)
source "$SCRIPT_DIR/lib/smoke_timeout.sh"

mksmoke_root() {
  python3 - "$1" "$2" <<'PY'
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
    raise SystemExit("SMOKE_ROOT must stay under the project root")
if resolved == project_root:
    raise SystemExit("SMOKE_ROOT cannot equal the project root")
print(resolved)
PY
}

RUNTIME_ID="${VISION_RUNTIME_ID:-vision.llava.preview}"
SMOKE_ROOT_RAW="${VISION_SMOKE_ROOT:-$PROJECT_ROOT/.smoke/vision}"
if ! SMOKE_ROOT="$(mksmoke_root "$SMOKE_ROOT_RAW" "$PROJECT_ROOT")"; then
  echo "[vision-smoke] invalid SMOKE_ROOT (${SMOKE_ROOT_RAW}); adjust VISION_SMOKE_ROOT." >&2
  exit 1
fi
mkdir -p "$SMOKE_ROOT"

prune_old_runs() {
  if [[ "${VISION_SMOKE_KEEP_TMP:-0}" = "1" ]] || [[ "${VISION_SMOKE_DISABLE_PRUNE:-0}" = "1" ]]; then
    return
  fi
  local root="$1"
  local keep="${VISION_SMOKE_KEEP_RECENT:-6}"
  local ttl="${VISION_SMOKE_RETENTION_SECS:-604800}"
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
    echo "[vision-smoke] pruned old runs: $removed" >&2
  fi
}

prune_old_runs "$SMOKE_ROOT"
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

CURL_MAX_TIME="${VISION_CURL_MAX_TIME:-5}"
CURL_CONNECT_TIMEOUT="${VISION_CURL_CONNECT_TIMEOUT:-3}"
CURL_RETRY="${VISION_CURL_RETRY:-2}"
CURL_RETRY_DELAY="${VISION_CURL_RETRY_DELAY:-1}"
CURL_ARGS_BASE=(-f -sS --max-time "$CURL_MAX_TIME")
if [[ "${CURL_CONNECT_TIMEOUT}" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  CURL_ARGS_BASE+=(--connect-timeout "$CURL_CONNECT_TIMEOUT")
fi
if [[ "${CURL_RETRY}" =~ ^[0-9]+$ && "${CURL_RETRY}" -gt 0 ]]; then
  CURL_ARGS_BASE+=(--retry "$CURL_RETRY")
  CURL_ARGS_BASE+=(--retry-connrefused)
  if [[ "${CURL_RETRY_DELAY}" =~ ^[0-9]+$ && "${CURL_RETRY_DELAY}" -gt 0 ]]; then
    CURL_ARGS_BASE+=(--retry-delay "$CURL_RETRY_DELAY")
  fi
fi

cleanup() {
  local status=$?
  status=$(smoke_timeout::cleanup "$status")
  if [[ -n "$SERVER_PID" ]]; then
    kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
    smoke_timeout::unregister_child "$SERVER_PID"
    SERVER_PID=""
  fi
  if [[ -n "${STUB_SCRIPT:-}" ]]; then
    if command -v pkill >/dev/null 2>&1; then
      pkill -f "$STUB_SCRIPT" 2>/dev/null || true
    else
      python3 - "$STUB_SCRIPT" <<'PY' 2>/dev/null || true
import os
import signal
import subprocess
import sys

needle = sys.argv[1]
try:
    output = subprocess.check_output(["ps", "-eo", "pid,command"], text=True)
except Exception:
    sys.exit(0)
for line in output.splitlines():
    parts = line.strip().split(None, 1)
    if len(parts) != 2:
        continue
    pid, command = parts
    if needle in command:
        try:
            os.kill(int(pid), signal.SIGTERM)
        except Exception:
            pass
PY
    fi
  fi
  if [[ $status -ne 0 ]]; then
    echo "[vision-smoke] server log (tail)" >&2
    tail -n 200 "$SERVER_LOG" >&2 || true
    if [[ -s "$STUB_LOG" ]]; then
      echo "[vision-smoke] stub log (tail)" >&2
      tail -n 200 "$STUB_LOG" >&2 || true
    fi
  fi
  if [[ "${VISION_SMOKE_KEEP_TMP:-0}" = "1" ]]; then
    echo "[vision-smoke] preserving run directory at ${TMP_DIR}" >&2
  else
    rm -rf "$TMP_DIR"
  fi
  return "$status"
}
trap cleanup EXIT

if [[ "${ARW_SMOKE_USE_SYNTHETIC:-0}" = "1" ]]; then
  echo "[vision-smoke] ARW_SMOKE_USE_SYNTHETIC=1 — skipping (no local sockets)." >&2
  exit 0
fi

ensure_server_bin() {
  local candidate="${ARW_SERVER_BIN:-}"
  if [[ -n "$candidate" ]]; then
    if [[ ! -x "$candidate" ]]; then
      echo "[vision-smoke] ARW_SERVER_BIN points to ${candidate}, but it is not executable." >&2
      exit 1
    fi
    printf '%s\n' "$candidate"
    return 0
  fi

  candidate="${PROJECT_ROOT}/target/debug/arw-server"
  if [[ ! -x "$candidate" ]]; then
    if ! command -v cargo >/dev/null 2>&1; then
      echo "[vision-smoke] cargo not found; set ARW_SERVER_BIN to a pre-built arw-server binary." >&2
      exit 1
    fi
    local build_log="$TMP_DIR/cargo-build.log"
    local cargo_args=(-p arw-server)
    if [[ -n "${VISION_CARGO_PROFILE:-}" ]]; then
      cargo_args+=(--profile "${VISION_CARGO_PROFILE}")
    fi
    if [[ -n "${VISION_CARGO_JOBS:-}" ]]; then
      cargo_args+=(--jobs "${VISION_CARGO_JOBS}")
    fi
    echo "[vision-smoke] building arw-server binary (cargo build ${cargo_args[*]})" >&2
    if ! (cd "$PROJECT_ROOT" && cargo build "${cargo_args[@]}" &>"$build_log"); then
      echo "[vision-smoke] cargo build failed; see ${build_log} for details." >&2
      tail -n 200 "$build_log" >&2 || true
      echo "[vision-smoke] supply ARW_SERVER_BIN to reuse an existing build or adjust VISION_CARGO_JOBS." >&2
      exit 1
    fi
  fi

  if [[ ! -x "$candidate" ]]; then
    echo "[vision-smoke] unable to locate arw-server binary at ${candidate}; set ARW_SERVER_BIN." >&2
    exit 1
  fi
  printf '%s\n' "$candidate"
}

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

  local server_bin
  server_bin="$(ensure_server_bin)"
  export ARW_SERVER_BIN="$server_bin"

  "$server_bin" >"$SERVER_LOG" 2>&1 &
  SERVER_PID=$!
  smoke_timeout::register_child "$SERVER_PID"

  local healthz="http://127.0.0.1:${ARW_PORT}/healthz"
  for _ in {1..240}; do
    if curl "${CURL_ARGS_BASE[@]}" "$healthz" >/dev/null 2>&1; then
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
  curl "${CURL_ARGS_BASE[@]}" "$base/state/runtime_matrix" \
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
detail = status.get("detail")
if not isinstance(detail, list) or not detail:
    raise SystemExit("runtime status missing detail entries")
if not all(isinstance(item, str) and item.strip() for item in detail):
    raise SystemExit("runtime status detail entries must be non-empty strings")
aria_hint = status.get("aria_hint")
if not isinstance(aria_hint, str) or not aria_hint.strip():
    raise SystemExit("runtime status missing aria_hint")
label = status.get("label")
if not isinstance(label, str) or not label.strip():
    raise SystemExit("runtime status missing label")
severity_label = status.get("severity_label")
if not isinstance(severity_label, str) or not severity_label.strip():
    raise SystemExit("runtime status missing severity_label")
runtime_meta = entry.get("runtime") or {}
updated = runtime_meta.get("updated")
if not isinstance(updated, str) or not updated.strip():
    raise SystemExit("runtime metadata missing updated timestamp")
ttl = data.get("ttl_seconds")
if not isinstance(ttl, (int, float)) or ttl <= 0:
    raise SystemExit("runtime matrix missing positive ttl_seconds")
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
  if ! curl "${CURL_ARGS_BASE[@]}" -X POST "http://127.0.0.1:${VISION_PORT}/describe" \
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
  curl "${CURL_ARGS_BASE[@]}" -X POST "$base/orchestrator/runtimes/${RUNTIME_ID}/restore" \
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
