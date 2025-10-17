#!/usr/bin/env python3
"""Cross-platform context telemetry smoke test."""

from __future__ import annotations

import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime
from pathlib import Path
from typing import Optional
from urllib.error import HTTPError, URLError
from urllib.request import Request, build_opener


def detect_repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def detect_host_mode() -> str:
    system = platform.system()
    if system == "Linux":
        try:
            with open("/proc/version", "r", encoding="utf-8", errors="ignore") as fh:
                version = fh.read().lower()
            if "microsoft" in version:
                return "windows-wsl"
        except FileNotFoundError:
            pass
        return "linux"
    if system == "Darwin":
        return "mac"
    if system.startswith(("CYGWIN", "MSYS", "Windows")):
        return "windows-host"
    return "unknown"


def read_mode_file(path: Path) -> Optional[str]:
    if not path.exists():
        return None
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.startswith("MODE="):
            return line.split("=", 1)[1].strip()
    return None


def ensure_mode(repo_root: Path) -> str:
    forced = os.environ.get("ARW_ENV_MODE_FORCE")
    if forced:
        mode = forced
    else:
        mode = os.environ.get("ARW_ENV_MODE") or read_mode_file(repo_root / ".arw-env")
    host = detect_host_mode()
    if not mode:
        mode = host
    if host != "unknown" and mode != host:
        raise SystemExit(
            f"[env-mode] Active environment mismatch: host={host} current={mode}. "
            "Run scripts/env/switch.* from the appropriate environment first."
        )
    os.environ["ARW_ENV_MODE"] = mode
    if host == "windows-host":
        suffix = ".exe"
    elif host == "windows-wsl":
        suffix = os.environ.get("ARW_EXE_SUFFIX", ".exe")
    else:
        suffix = ""
    os.environ.setdefault("ARW_EXE_SUFFIX", suffix)
    return suffix


def ensure_admin_token() -> str:
    token = os.environ.get("ARW_ADMIN_TOKEN")
    if token:
        return token
    token = "context-ci-token"
    os.environ["ARW_ADMIN_TOKEN"] = token
    os.environ["ARW_ADMIN_TOKEN_SHA256"] = hashlib.sha256(token.encode("utf-8")).hexdigest()
    return token


def ensure_debug_server(repo_root: Path, server_bin: Path) -> None:
    if server_bin.is_file():
        return
    proc = subprocess.run(
        ["cargo", "build", "-p", "arw-server"],
        cwd=repo_root,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    if proc.returncode != 0:
        print(proc.stdout, file=sys.stderr)
        raise SystemExit(proc.returncode)


def wait_for_health(base: str, timeout: int, server_proc: subprocess.Popen, log_path: Path) -> None:
    deadline = time.time() + timeout
    opener = build_opener()
    while time.time() < deadline:
        try:
            with opener.open(Request(f"{base}/healthz"), timeout=2):
                return
        except URLError:
            pass
        time.sleep(1)
        if server_proc.poll() is not None:
            print("context-ci: server exited before becoming healthy", file=sys.stderr)
            if log_path.exists():
                for line in log_path.read_text(encoding="utf-8", errors="ignore").splitlines():
                    print(f"[arw-server] {line}", file=sys.stderr)
            raise SystemExit(1)
    if log_path.exists():
        for line in log_path.read_text(encoding="utf-8", errors="ignore").splitlines():
            print(f"[arw-server] {line}", file=sys.stderr)
    raise SystemExit("context-ci: /healthz did not respond within timeout")


def run_telemetry_check(base: str, token: str) -> None:
    opener = build_opener()

    def attach(req: Request) -> Request:
        if token:
            req.add_header("Authorization", f"Bearer {token}")
        return req

    def submit(msg: str) -> str:
        payload = json.dumps({"kind": "demo.echo", "input": {"msg": msg}}).encode("utf-8")
        req = Request(f"{base}/actions", data=payload, method="POST")
        req.add_header("Content-Type", "application/json")
        attach(req)
        with opener.open(req, timeout=10) as resp:
            reply = json.load(resp)
        action_id = reply.get("id") or (reply.get("action") or {}).get("id")
        if not action_id:
            raise RuntimeError(f"missing action id in {reply}")
        return action_id

    def wait_complete(action_id: str) -> None:
        deadline = time.time() + 20
        last_state = None
        while time.time() < deadline:
            req = Request(f"{base}/actions/{action_id}")
            attach(req)
            try:
                with opener.open(req, timeout=10) as resp:
                    doc = json.load(resp)
            except HTTPError as err:
                if err.code == 404:
                    time.sleep(0.5)
                    continue
                raise
            state = doc.get("state")
            if state == "completed":
                return
            last_state = state
            if state in {"queued", "running"}:
                time.sleep(0.5)
                continue
            raise RuntimeError(f"unexpected action state {state}: {doc}")
        raise RuntimeError(f"action {action_id} did not complete in time (last state {last_state})")

    for idx in range(2):
        action_id = submit(f"context-ci-{idx}")
        wait_complete(action_id)

    req = Request(f"{base}/state/training/telemetry")
    attach(req)
    with opener.open(req, timeout=10) as resp:
        telemetry = json.load(resp)

    for key in ["generated", "events", "routes", "bus", "tools"]:
        if key not in telemetry:
            raise SystemExit(f"telemetry missing {key}: {telemetry}")

    try:
        datetime.fromisoformat(telemetry["generated"].replace("Z", "+00:00"))
    except Exception as exc:  # noqa: BLE001
        raise SystemExit(f"telemetry generated timestamp invalid: {exc}")

    events = telemetry.get("events")
    if not isinstance(events, dict) or "total" not in events or events.get("total", 0) < 2:
        raise SystemExit(f"telemetry events malformed: {events}")

    routes = telemetry.get("routes")
    if not isinstance(routes, list):
        raise SystemExit(f"telemetry routes malformed: {routes}")

    bus = telemetry.get("bus")
    if not isinstance(bus, dict) or "published" not in bus:
        raise SystemExit(f"telemetry bus malformed: {bus}")

    tools = telemetry.get("tools")
    if not isinstance(tools, dict) or tools.get("completed", 0) < 2:
        raise SystemExit(f"telemetry tools did not record completions: {tools}")

    print("context-ci OK — telemetry snapshot includes recent runs")


def main() -> None:
    repo_root = detect_repo_root()
    suffix = ensure_mode(repo_root)

    port = int(os.environ.get("ARW_CONTEXT_CI_PORT", "18182"))
    token = ensure_admin_token()
    os.environ["ARW_CONTEXT_CI_TOKEN"] = token

    state_dir = Path(tempfile.mkdtemp(prefix="context-ci-state-"))
    log_fd, log_path_str = tempfile.mkstemp(prefix="context-ci-log-")
    os.close(log_fd)
    log_path = Path(log_path_str)

    server_bin = repo_root / "target" / "debug" / f"arw-server{suffix}"
    ensure_debug_server(repo_root, server_bin)

    env = os.environ.copy()
    env.update(
        {
            "ARW_PORT": str(port),
            "ARW_STATE_DIR": str(state_dir),
            "ARW_DEBUG": "0",
        }
    )

    server_log_handle = open(log_path, "w", encoding="utf-8")
    server_proc = subprocess.Popen(
        [str(server_bin), "--port", str(port)],
        env=env,
        cwd=repo_root,
        stdout=server_log_handle,
        stderr=subprocess.STDOUT,
    )

    base = f"http://127.0.0.1:{port}"
    try:
        wait_for_health(base, 30, server_proc, log_path)
        run_telemetry_check(base, token)
    finally:
        try:
            server_proc.terminate()
            server_proc.wait(timeout=5)
        except Exception:
            server_proc.kill()
        server_log_handle.close()
        shutil.rmtree(state_dir, ignore_errors=True)
        try:
            os.remove(log_path)
        except OSError:
            pass


if __name__ == "__main__":
    main()
