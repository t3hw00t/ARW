#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT_DIR=$(cd "$SCRIPT_DIR/.." && pwd)
REPO_ROOT="$ROOT_DIR"
source "$REPO_ROOT/scripts/lib/env_mode.sh"
arw_env_init
source "$SCRIPT_DIR/lib/smoke_timeout.sh"
smoke_timeout::init "triad-smoke" 600 "TRIAD_SMOKE_TIMEOUT_SECS"

is_truthy() {
  case "$(echo "${1:-}" | tr '[:upper:]' '[:lower:]')" in
    1|true|yes|on) return 0 ;;
    *) return 1 ;;
  esac
}

ADMIN_TOKEN="${ARW_TRIAD_SMOKE_ADMIN_TOKEN:-triad-smoke-token}"
# Optional: set TRIAD_SMOKE_PERSONA / SMOKE_TRIAD_PERSONA / ARW_PERSONA_ID to tag synthetic smoke actions.
PERSONA_ID="${TRIAD_SMOKE_PERSONA:-${SMOKE_TRIAD_PERSONA:-${ARW_PERSONA_ID:-}}}"
AUTH_MODE="$(echo "${TRIAD_SMOKE_AUTH_MODE:-bearer}" | tr '[:upper:]' '[:lower:]')"
HEALTHZ_HEADER_RAW="${TRIAD_SMOKE_HEALTHZ_HEADER:-${TRIAD_SMOKE_AUTH_HEADER:-}}"
HEALTHZ_BEARER_OVERRIDE="${TRIAD_SMOKE_HEALTHZ_BEARER:-}"
TLS_CERT="${TRIAD_SMOKE_TLS_CERT:-}"
TLS_KEY="${TRIAD_SMOKE_TLS_KEY:-}"
TLS_CA="${TRIAD_SMOKE_TLS_CA:-}"
BASIC_USER="${TRIAD_SMOKE_BASIC_USER:-}"
BASIC_PASSWORD="${TRIAD_SMOKE_BASIC_PASSWORD:-}"

BASE_OVERRIDE="${TRIAD_SMOKE_BASE_URL:-${SMOKE_TRIAD_BASE_URL:-}}"
STATE_DIR=""
LOG_FILE=""
SERVER_PID=""
START_SERVER=1

if [[ -n "$BASE_OVERRIDE" ]]; then
  START_SERVER=0
else
  PORT="${ARW_TRIAD_SMOKE_PORT:-18181}"
  STATE_DIR="$(mktemp -d)"
  LOG_FILE="$(mktemp)"
  exe_suffix="${ARW_EXE_SUFFIX:-}"
  SERVER_BIN="${ROOT_DIR}/target/debug/arw-server${exe_suffix}"
  BUILD_PROFILE="debug"

  release_candidate="${ROOT_DIR}/target/release/arw-server${exe_suffix}"
  if is_truthy "${RUNTIME_SMOKE_USE_RELEASE:-0}"; then
    SERVER_BIN="$release_candidate"
    BUILD_PROFILE="release"
  elif [[ ! -x "$SERVER_BIN" && -x "$release_candidate" ]]; then
    SERVER_BIN="$release_candidate"
    BUILD_PROFILE="release"
  fi
fi

cleanup() {
  local status=$?
  status=$(smoke_timeout::cleanup "$status")
  if [[ $START_SERVER -eq 1 && -n "${SERVER_PID:-}" ]]; then
    kill "${SERVER_PID}" >/dev/null 2>&1 || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
  if [[ -n "$STATE_DIR" ]]; then
    rm -rf "${STATE_DIR}" 2>/dev/null || true
  fi
  if [[ -n "$LOG_FILE" ]]; then
    rm -f "${LOG_FILE}" 2>/dev/null || true
  fi
  return "$status"
}
trap cleanup EXIT

healthz_curl_opts=(-fsS)
custom_auth_header=0
if [[ -n "$HEALTHZ_HEADER_RAW" ]]; then
  healthz_curl_opts+=(-H "$HEALTHZ_HEADER_RAW")
  shopt -s nocasematch
  if [[ "$HEALTHZ_HEADER_RAW" == authorization:* ]]; then
    custom_auth_header=1
  fi
  shopt -u nocasematch
fi
if [[ "$AUTH_MODE" == "basic" ]]; then
  basic_user="$BASIC_USER"
  basic_pass="$BASIC_PASSWORD"
  if [[ -z "$basic_user" && "$ADMIN_TOKEN" == *:* ]]; then
    basic_user="${ADMIN_TOKEN%%:*}"
    basic_pass="${ADMIN_TOKEN#*:}"
  fi
  basic_user="${basic_user:-}"
  basic_pass="${basic_pass:-}"
  basic_token=$(printf '%s:%s' "$basic_user" "$basic_pass" | base64 | tr -d '\r\n')
  healthz_curl_opts+=(-H "Authorization: Basic $basic_token")
  custom_auth_header=1
elif [[ -n "$HEALTHZ_BEARER_OVERRIDE" ]]; then
  healthz_curl_opts+=(-H "Authorization: Bearer $HEALTHZ_BEARER_OVERRIDE")
  custom_auth_header=1
elif [[ $custom_auth_header -eq 0 && -n "$ADMIN_TOKEN" ]]; then
  healthz_curl_opts+=(-H "Authorization: Bearer $ADMIN_TOKEN")
fi
if [[ -n "$TLS_CERT" ]]; then
  healthz_curl_opts+=(--cert "$TLS_CERT")
  if [[ -n "$TLS_KEY" ]]; then
    healthz_curl_opts+=(--key "$TLS_KEY")
  fi
elif [[ -n "$TLS_KEY" ]]; then
  echo "triad-smoke: TRIAD_SMOKE_TLS_KEY set without TRIAD_SMOKE_TLS_CERT" >&2
  exit 1
fi
if [[ -n "$TLS_CA" ]]; then
  healthz_curl_opts+=(--cacert "$TLS_CA")
fi

healthz_check() {
  local base_url=$1
  curl "${healthz_curl_opts[@]}" "${base_url}/healthz" >/dev/null 2>&1
}

if [[ $START_SERVER -eq 1 ]]; then
  if [[ ! -x "$SERVER_BIN" ]]; then
    build_log="$(mktemp -t triad-smoke-build.XXXX.log)"
    echo "[triad-smoke] building arw-server binary ($BUILD_PROFILE)" >&2
    build_cmd=(cargo build -p arw-server)
    if [[ "$BUILD_PROFILE" == "release" ]]; then
      build_cmd+=(--release)
    fi
    if ! (cd "$ROOT_DIR" && "${build_cmd[@]}" &>"$build_log"); then
      echo "[triad-smoke] cargo build failed; log preserved at $build_log" >&2
      tail -n 200 "$build_log" >&2 || true
      exit 1
    fi
    rm -f "$build_log"
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
    if healthz_check "$BASE"; then
      break
    fi
    if ! kill -0 "$SERVER_PID" >/dev/null 2>&1; then
      sed 's/^/[arw-server] /' "$LOG_FILE" >&2 || true
      echo "triad-smoke: server exited before becoming healthy" >&2
      exit 1
    fi
    sleep 1
  done

  if ! healthz_check "$BASE"; then
    sed 's/^/[arw-server] /' "$LOG_FILE" >&2 || true
    echo "triad-smoke: /healthz did not respond within timeout" >&2
    exit 1
  fi
else
  BASE="$BASE_OVERRIDE"
  if [[ -z "$BASE" ]]; then
    echo "triad-smoke: TRIAD_SMOKE_BASE_URL must be set when not starting a server" >&2
    exit 1
  fi
  if ! healthz_check "$BASE"; then
    echo "triad-smoke: remote base $BASE failed /healthz check" >&2
    exit 1
  fi
fi

python3 - "$BASE" "$ADMIN_TOKEN" "$PERSONA_ID" <<'PY'
import base64
import json
import os
import ssl
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime

base = sys.argv[1]
token = sys.argv[2]
persona = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] else None

auth_mode = os.environ.get("TRIAD_SMOKE_AUTH_MODE", "bearer").strip().lower()
basic_user_env = os.environ.get("TRIAD_SMOKE_BASIC_USER")
basic_pass_env = os.environ.get("TRIAD_SMOKE_BASIC_PASSWORD")
custom_auth_header = os.environ.get("TRIAD_SMOKE_AUTH_HEADER") or os.environ.get(
    "TRIAD_SMOKE_HEALTHZ_HEADER"
)
client_cert = os.environ.get("TRIAD_SMOKE_TLS_CERT")
client_key = os.environ.get("TRIAD_SMOKE_TLS_KEY")
ca_bundle = os.environ.get("TRIAD_SMOKE_TLS_CA")

ssl_context = None
if client_cert or client_key or ca_bundle:
    ssl_context = ssl.create_default_context(cafile=ca_bundle) if ca_bundle else ssl.create_default_context()
    try:
        if client_cert:
            ssl_context.load_cert_chain(certfile=client_cert, keyfile=client_key if client_key else None)
    except Exception as exc:  # noqa: BLE001
        raise SystemExit(f"failed to load client certificate: {exc}")

handlers = []
if ssl_context is not None:
    handlers.append(urllib.request.HTTPSHandler(context=ssl_context))
opener = urllib.request.build_opener(*handlers)


def apply_auth(req: urllib.request.Request) -> None:
    mode = auth_mode or "bearer"
    if mode == "none":
        return
    if mode == "bearer" or mode not in {"bearer", "basic", "header", "none"}:
        if token:
            req.add_header("Authorization", f"Bearer {token}")
        return
    if mode == "basic":
        user = basic_user_env
        password = basic_pass_env
        if (not user) and token and ":" in token:
            user, password = token.split(":", 1)
        user = (user or "").strip()
        password = password or ""
        creds = f"{user}:{password}".encode("utf-8")
        header_value = base64.b64encode(creds).decode("ascii")
        req.add_header("Authorization", f"Basic {header_value}")
        return
    if mode == "header" and custom_auth_header and ":" in custom_auth_header:
        key, value = custom_auth_header.split(":", 1)
        req.add_header(key.strip(), value.strip())


def request(path, *, method="GET", data=None, headers=None, timeout=10):
    req = urllib.request.Request(f"{base}{path}", data=data, method=method)
    apply_auth(req)
    if data is not None:
        req.add_header("content-type", "application/json")
    if headers:
        for key, value in headers.items():
            req.add_header(key, value)
    return opener.open(req, timeout=timeout)


def ensure_action_roundtrip():
    request_body = {"kind": "demo.echo", "input": {"msg": "triad-smoke"}}
    if persona:
        request_body["persona_id"] = persona
    payload = json.dumps(request_body).encode()
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
            if persona:
                persona_in_status = status_doc.get("persona_id")
                if persona_in_status and persona_in_status != persona:
                    raise SystemExit(
                        f"persona mismatch: expected {persona!r}, got {persona_in_status!r}"
                    )
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

if persona:
    print(
        f"triad-smoke OK — action {action_id} completed; persona {persona}; state + events healthy"
    )
else:
    print(f"triad-smoke OK — action {action_id} completed; state + events healthy")
PY
