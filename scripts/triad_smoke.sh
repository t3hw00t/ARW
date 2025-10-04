#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT_DIR=$(cd "$SCRIPT_DIR/.." && pwd)
source "$SCRIPT_DIR/lib/smoke_timeout.sh"
smoke_timeout::init "triad-smoke" 600 "TRIAD_SMOKE_TIMEOUT_SECS"

PORT="${ARW_TRIAD_SMOKE_PORT:-18181}"
STATE_DIR="$(mktemp -d)"
LOG_FILE="$(mktemp)"
SERVER_BIN="${ROOT_DIR}/target/debug/arw-server"
ADMIN_TOKEN="${ARW_TRIAD_SMOKE_ADMIN_TOKEN:-triad-smoke-token}"

cleanup() {
  local status=$?
  status=$(smoke_timeout::cleanup "$status")
  if [[ -n "${SERVER_PID:-}" ]]; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  rm -rf "${STATE_DIR}" "${LOG_FILE}" 2>/dev/null || true
  return "$status"
}
trap cleanup EXIT

if [[ ! -x "$SERVER_BIN" ]]; then
  (cd "$ROOT_DIR" && cargo build -p arw-server >/dev/null 2>&1)
fi

ARW_PORT="$PORT" \
ARW_STATE_DIR="$STATE_DIR" \
ARW_ADMIN_TOKEN="$ADMIN_TOKEN" \
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

python3 - "$BASE" "$ADMIN_TOKEN" <<'PY'
import json
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime

base = sys.argv[1]
token = sys.argv[2]

opener = urllib.request.build_opener()


def request(path, *, method="GET", data=None, headers=None, timeout=10):
    req = urllib.request.Request(f"{base}{path}", data=data, method=method)
    req.add_header("Authorization", f"Bearer {token}")
    if data is not None:
        req.add_header("content-type", "application/json")
    if headers:
        for key, value in headers.items():
            req.add_header(key, value)
    return opener.open(req, timeout=timeout)


def ensure_action_roundtrip():
    payload = json.dumps({"kind": "demo.echo", "input": {"msg": "triad-smoke"}}).encode()
    try:
        with request("/actions", method="POST", data=payload) as resp:
            submit = json.loads(resp.read())
    except Exception as exc:  # noqa: BLE001
        raise SystemExit(f"failed to submit action: {exc}")

    action_id = submit.get("id") or submit.get("action", {}).get("id")
    if not action_id:
        raise SystemExit(f"response missing action id: {submit}")

    status = None
    deadline = time.time() + 20
    while time.time() < deadline:
        try:
            with request(f"/actions/{action_id}") as resp:
                status_doc = json.loads(resp.read())
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
            echo = output.get("echo") if isinstance(output, dict) else None
            if echo is None:
                raise SystemExit(f"completed without echo payload: {status_doc}")
            created = status_doc.get("created")
            if created:
                try:
                    datetime.fromisoformat(created.replace("Z", "+00:00"))
                except Exception as exc:  # noqa: BLE001
                    raise SystemExit(f"invalid created timestamp: {exc}")
            return action_id
        if status in {"queued", "running"}:
            time.sleep(0.5)
            continue
        raise SystemExit(f"unexpected action state {status}: {status_doc}")

    raise SystemExit(f"action {action_id} did not complete in time (last state {status})")


def ensure_projects_snapshot():
    try:
        with request("/state/projects") as resp:
            doc = json.loads(resp.read())
    except Exception as exc:  # noqa: BLE001
        raise SystemExit(f"failed to fetch /state/projects: {exc}")
    if not isinstance(doc, dict):
        raise SystemExit(f"unexpected /state/projects payload: {doc!r}")
    if "generated" not in doc or "items" not in doc:
        raise SystemExit(f"missing keys in /state/projects response: {doc}")


def ensure_sse_handshake(last_event_id=None):
    headers = {"Accept": "text/event-stream"}
    if last_event_id is not None:
        headers["Last-Event-ID"] = last_event_id
    try:
        with request("/events?replay=1", headers=headers, timeout=6) as resp:
            line = resp.readline().decode("utf-8", "ignore")
            buffer = line
            attempts = 0
            while "event: service.connected" not in buffer and attempts < 5:
                chunk = resp.readline().decode("utf-8", "ignore")
                if not chunk:
                    break
                buffer += chunk
                attempts += 1
    except Exception as exc:  # noqa: BLE001
        raise SystemExit(f"failed to open SSE stream: {exc}")
    if "event: service.connected" not in buffer:
        raise SystemExit(f"did not observe service.connected handshake (buffer={buffer!r})")


action_id = ensure_action_roundtrip()
ensure_projects_snapshot()
ensure_sse_handshake()
ensure_sse_handshake(last_event_id="0")

print(f"triad-smoke OK — action {action_id} completed; state + events healthy")
PY
