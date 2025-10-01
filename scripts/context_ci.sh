#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${ARW_CONTEXT_CI_PORT:-18182}"
STATE_DIR="$(mktemp -d)"
LOG_FILE="$(mktemp)"
SERVER_BIN="${ROOT_DIR}/target/debug/arw-server"

# Ensure we have an admin token so guarded endpoints succeed locally.
ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-}"
if [[ -z "$ADMIN_TOKEN" ]]; then
  ADMIN_TOKEN="context-ci-token"
  export ARW_ADMIN_TOKEN="$ADMIN_TOKEN"
  if command -v sha256sum >/dev/null 2>&1; then
    export ARW_ADMIN_TOKEN_SHA256="$(printf '%s' "$ADMIN_TOKEN" | sha256sum | cut -d' ' -f1)"
  elif command -v shasum >/dev/null 2>&1; then
    export ARW_ADMIN_TOKEN_SHA256="$(printf '%s' "$ADMIN_TOKEN" | shasum -a 256 | cut -d' ' -f1)"
  fi
fi
export ARW_CONTEXT_CI_TOKEN="$ADMIN_TOKEN"

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
    echo "context-ci: server exited before becoming healthy" >&2
    exit 1
  fi
  sleep 1
done

if ! curl -fsS "$BASE/healthz" >/dev/null 2>&1; then
  sed 's/^/[arw-server] /' "$LOG_FILE" >&2 || true
  echo "context-ci: /healthz did not respond within timeout" >&2
  exit 1
fi

python3 - "$BASE" <<'PY'
import json
import os
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime

base = sys.argv[1]
opener = urllib.request.build_opener()
ADMIN_TOKEN = os.environ.get("ARW_CONTEXT_CI_TOKEN")


def attach_admin(req: urllib.request.Request) -> urllib.request.Request:
    if ADMIN_TOKEN:
        req.add_header("authorization", f"Bearer {ADMIN_TOKEN}")
    return req


def submit(msg: str) -> str:
    payload = json.dumps({"kind": "demo.echo", "input": {"msg": msg}}).encode()
    req = urllib.request.Request(f"{base}/actions", data=payload, method="POST")
    req.add_header("content-type", "application/json")
    attach_admin(req)
    with opener.open(req, timeout=10) as resp:
        reply = json.loads(resp.read())
    action_id = reply.get("id") or reply.get("action", {}).get("id")
    if not action_id:
        raise RuntimeError(f"missing action id in {reply}")
    return action_id


def wait_complete(action_id: str) -> None:
    deadline = time.time() + 20
    last_state = None
    while time.time() < deadline:
        try:
            req = urllib.request.Request(f"{base}/actions/{action_id}")
            attach_admin(req)
            with opener.open(req, timeout=10) as resp:
                doc = json.loads(resp.read())
        except urllib.error.HTTPError as http_err:
            if http_err.code == 404:
                time.sleep(0.5)
                continue
            raise
        if doc.get("state") == "completed":
            return
        last_state = doc.get("state")
        if last_state in {"queued", "running"}:
            time.sleep(0.5)
            continue
        raise RuntimeError(f"unexpected state {last_state}: {doc}")
    raise RuntimeError(f"action {action_id} did not complete (last state {last_state})")


# run a couple of actions to populate metrics
ids = [submit(f"context-ci-{idx}") for idx in range(2)]
for action_id in ids:
    wait_complete(action_id)
req = urllib.request.Request(f"{base}/state/training/telemetry")
attach_admin(req)
with opener.open(req, timeout=10) as resp:
    telemetry = json.loads(resp.read())

required_root = ["generated", "events", "routes", "bus", "tools"]
for key in required_root:
    if key not in telemetry:
        raise SystemExit(f"telemetry missing {key}: {telemetry}")

try:
    datetime.fromisoformat(telemetry["generated"].replace("Z", "+00:00"))
except Exception as exc:  # noqa: BLE001
    raise SystemExit(f"telemetry generated timestamp invalid: {exc}")

events = telemetry["events"]
if not isinstance(events, dict) or "total" not in events:
    raise SystemExit(f"telemetry events malformed: {events}")
if events.get("total", 0) < 2:
    raise SystemExit(f"telemetry events did not record actions: {events}")

routes = telemetry.get("routes", [])
if not isinstance(routes, list):
    raise SystemExit(f"telemetry routes malformed: {routes}")

bus = telemetry.get("bus", {})
if not isinstance(bus, dict) or "published" not in bus:
    raise SystemExit(f"telemetry bus malformed: {bus}")

tools = telemetry.get("tools", {})
if tools.get("completed", 0) < 2:
    raise SystemExit(f"telemetry tools did not record completions: {tools}")

print("context-ci OK — telemetry snapshot includes recent runs")
PY
