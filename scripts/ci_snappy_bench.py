#!/usr/bin/env python3
"""
Run the snappy bench smoke in a cross-platform manner.

This script replaces the Bash implementation so Windows hosts no longer rely on
Git Bash. It mirrors the original behaviour:
  - ensures release binaries exist (rebuilding when the bench CLI signature drifts)
  - launches arw-server in the background
  - waits for /healthz
  - executes snappy-bench with JSON output
  - enforces queue/total latency budgets
"""

from __future__ import annotations

import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Optional
from urllib.request import urlopen
from urllib.error import URLError


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


def ensure_mode(reporoot: Path) -> str:
    forced = os.environ.get("ARW_ENV_MODE_FORCE")
    if forced:
        mode = forced
    else:
        mode = os.environ.get("ARW_ENV_MODE") or read_mode_file(reporoot / ".arw-env")
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


def resolve_python() -> str:
    return sys.executable


def ensure_release_binaries(root: Path, server_bin: Path, bench_bin: Path) -> None:
    need_build = False
    if not server_bin.is_file() or not bench_bin.is_file():
        need_build = True
    else:
        try:
            res = subprocess.run(
                [str(bench_bin), "--help"],
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                text=True,
                check=False,
            )
            if "--json-out" not in res.stdout:
                print("[snappy-bench] installed bench missing --json-out; forcing rebuild")
                need_build = True
        except FileNotFoundError:
            need_build = True

    if need_build:
        log_path = Path(tempfile.gettempdir()) / "snappy-bench-build.log"
        print("[snappy-bench] building release binaries")
        with log_path.open("w", encoding="utf-8") as log:
            proc = subprocess.run(
                [
                    "cargo",
                    "build",
                    "--release",
                    "-p",
                    "arw-server",
                    "-p",
                    "snappy-bench",
                ],
                cwd=root,
                stdout=log,
                stderr=subprocess.STDOUT,
                text=True,
            )
        if proc.returncode != 0:
            print("[snappy-bench] build failed", file=sys.stderr)
            try:
                content = log_path.read_text(encoding="utf-8")
                for line in content.splitlines():
                    print(f"[build] {line}", file=sys.stderr)
            except OSError:
                pass
            raise SystemExit(proc.returncode)


def wait_for_health(port: int, timeout: int, server_pid: int, log_path: Path) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            with urlopen(f"http://127.0.0.1:{port}/healthz", timeout=2):
                return
        except URLError:
            pass
        time.sleep(1)
        try:
            os.kill(server_pid, 0)
        except OSError:
            print("[snappy-bench] server exited early", file=sys.stderr)
            if log_path.exists():
                for line in log_path.read_text(encoding="utf-8", errors="ignore").splitlines():
                    print(f"[server] {line}", file=sys.stderr)
            raise SystemExit(1)

    print(f"[snappy-bench] server failed to become healthy within {timeout}s", file=sys.stderr)
    if log_path.exists():
        for line in log_path.read_text(encoding="utf-8", errors="ignore").splitlines():
            print(f"[server] {line}", file=sys.stderr)
    raise SystemExit(1)


def parse_summary(json_path: Path) -> dict:
    with json_path.open("r", encoding="utf-8") as fh:
        data = json.load(fh)
    latency = data.get("latency_ms") or {}
    return {
        "requests": data.get("requests"),
        "completed": data.get("completed"),
        "failed": data.get("failed"),
        "queue_p95_ms": ((latency.get("queue") or {}).get("p95")),
        "total_p95_ms": ((latency.get("total") or {}).get("p95")),
        "throughput_per_sec": data.get("throughput_per_sec"),
    }


def main() -> None:
    repo_root = detect_repo_root()
    suffix = ensure_mode(repo_root)

    port = int(os.environ.get("ARW_BENCH_PORT", "8091"))
    token = os.environ.get("ARW_BENCH_TOKEN", "ci-bench")
    requests = os.environ.get("ARW_BENCH_REQUESTS", "60")
    concurrency = os.environ.get("ARW_BENCH_CONCURRENCY", "6")
    wait_timeout = int(os.environ.get("ARW_BENCH_HEALTH_TIMEOUT", "30"))
    queue_budget = float(os.environ.get("ARW_BENCH_QUEUE_BUDGET_MS", "500"))
    total_budget = float(os.environ.get("ARW_BENCH_FULL_BUDGET_MS", "2000"))

    state_dir = Path(tempfile.mkdtemp(prefix="snappy-bench-state-"))

    runner_temp = os.environ.get("RUNNER_TEMP")
    if runner_temp:
        bench_json = Path(runner_temp) / "snappy-bench-summary.json"
    else:
        fd, tmp_path = tempfile.mkstemp(prefix="snappy-bench-", suffix=".json")
        os.close(fd)
        bench_json = Path(tmp_path)

    server_bin = repo_root / "target" / "release" / f"arw-server{suffix}"
    bench_bin = repo_root / "target" / "release" / f"snappy-bench{suffix}"

    ensure_release_binaries(repo_root, server_bin, bench_bin)

    server_log = Path(tempfile.gettempdir()) / "snappy-bench-server.log"

    env = os.environ.copy()
    env.update(
        {
            "ARW_ADMIN_TOKEN": token,
            "ARW_PORT": str(port),
            "ARW_DEBUG": "0",
            "ARW_STATE_DIR": str(state_dir),
        }
    )

    server_log_handle = server_log.open("w", encoding="utf-8")
    server_proc = subprocess.Popen(
        [str(server_bin), "--port", str(port)],
        stdout=server_log_handle,
        stderr=subprocess.STDOUT,
        env=env,
        cwd=repo_root,
    )
    print(f"[snappy-bench] arw-server spawned (pid={server_proc.pid}), waiting for healthz...")
    try:
        wait_for_health(port, wait_timeout, server_proc.pid, server_log)
        print(f"[snappy-bench] server healthy, running bench (requests={requests}, concurrency={concurrency})")
        bench_cmd = [
            str(bench_bin),
            "--base",
            f"http://127.0.0.1:{port}",
            "--admin-token",
            token,
            "--requests",
            str(requests),
            "--concurrency",
            str(concurrency),
            "--json-out",
            str(bench_json),
            "--budget-queue-ms",
            str(int(queue_budget)),
            "--budget-full-ms",
            str(int(total_budget)),
        ]
        result = subprocess.run(bench_cmd, env=env, cwd=repo_root)
        if result.returncode != 0:
            raise SystemExit(result.returncode)

        print("[snappy-bench] bench run completed")
        summary = parse_summary(bench_json)
        print("[snappy-bench] parsed summary:")
        print(json.dumps(summary, indent=2))

        if summary["total_p95_ms"] is not None and summary["total_p95_ms"] > total_budget:
            raise SystemExit("total p95 exceeded budget")
        if summary["queue_p95_ms"] is not None and summary["queue_p95_ms"] > queue_budget:
            raise SystemExit("queue p95 exceeded budget")

        print(f"[snappy-bench] JSON summary stored in {bench_json}")
    finally:
        try:
            server_proc.terminate()
            server_proc.wait(timeout=5)
        except Exception:
            server_proc.kill()
        finally:
            try:
                server_log_handle.close()
            except Exception:
                pass
        if runner_temp is None:
            try:
                bench_json.unlink(missing_ok=True)  # type: ignore[arg-type]
            except Exception:
                pass
        shutil.rmtree(state_dir, ignore_errors=True)


if __name__ == "__main__":
    main()
