#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${ARW_TRIAD_SMOKE_PORT:-18181}"
STATE_DIR="$(mktemp -d)"
LOG_FILE="$(mktemp)"
SERVER_BIN="${ROOT_DIR}/target/debug/arw-server"

cleanup() {
  local status=$?
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  rm -rf "${STATE_DIR}" "${LOG_FILE}" 2>/dev/null || true
  return $status
}
trap cleanup EXIT

if [[ ! -x "$SERVER_BIN" ]]; then
  (cd "$ROOT_DIR" && cargo build -p arw-server >/dev/null 2>&1)
fi

ARW_PORT="$PORT" \
ARW_STATE_DIR="$STATE_DIR" \
ARW_DEBUG=0 \
"$SERVER_BIN" >"$LOG_FILE" 2>&1 &
SERVER_PID=$!

BASE="http://127.0.0.1:${PORT}"
DEADLINE=$(( $(date +%s) + 30 ))
while [[ $(date +%s) -lt $DEADLINE ]]; do
  if curl -fsS "$BASE/healthz" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
    sed 's/^/[arw-server] /' "$LOG_FILE" >&2 || true
    echo "triad-smoke: server exited before becoming healthy" >&2
    exit 1
  fi
  sleep 1
done

if ! curl -fsS "$BASE/healthz" >/dev/null 2>&1; then
  sed 's/^/[arw-server] /' "$LOG_FILE" >&2 || true
  echo "triad-smoke: /healthz did not respond within timeout" >&2
  exit 1
fi

python3 - "$BASE" <<'PY'
import json
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime

base = sys.argv[1]

opener = urllib.request.build_opener()

payload = json.dumps({"kind": "demo.echo", "input": {"msg": "triad-smoke"}}).encode()
try:
    req = urllib.request.Request(f"{base}/actions", data=payload, method="POST")
    req.add_header("content-type", "application/json")
    with opener.open(req, timeout=10) as resp:
        body = resp.read()
    submit = json.loads(body)
except Exception as exc:  # noqa: BLE001
    raise SystemExit(f"failed to submit action: {exc}")

action_id = submit.get("id") or submit.get("action", {}).get("id")
if not action_id:
    raise SystemExit(f"response missing action id: {submit}")

status = None
deadline = time.time() + 20
while time.time() < deadline:
    try:
        with opener.open(f"{base}/actions/{action_id}", timeout=10) as resp:
            body = resp.read()
        status_doc = json.loads(body)
    except urllib.error.HTTPError as http_err:
        if http_err.code == 404:
            time.sleep(0.5)
            continue
        raise SystemExit(f"actions/{action_id} failed: {http_err}")
    except Exception as exc:  # noqa: BLE001
        raise SystemExit(f"failed to fetch action status: {exc}")

    status = status_doc.get("state")
    if status == "completed":
        output = status_doc.get("output", {})
        echo = None
        if isinstance(output, dict):
            echo = output.get("echo")
        if echo is None:
            raise SystemExit(f"completed without echo payload: {status_doc}")
        # ensure generated timestamp is ISO8601 when present
        if status_doc.get("created"):
            try:
                datetime.fromisoformat(status_doc["created"].replace("Z", "+00:00"))
            except Exception as exc:  # noqa: BLE001
                raise SystemExit(f"invalid created timestamp: {exc}")
        print(f"triad-smoke OK — action {action_id} completed")
        sys.exit(0)
    if status in {"queued", "running"}:
        time.sleep(0.5)
        continue
    raise SystemExit(f"unexpected action state {status}: {status_doc}")

raise SystemExit(f"action {action_id} did not complete in time (last state {status})")
PY
