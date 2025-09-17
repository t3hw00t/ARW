#!/usr/bin/env python3
"""Shared helpers for documentation generators."""
from __future__ import annotations

import datetime
import os
import pathlib
import re
import subprocess
from typing import Iterable, List, Sequence, Set

ROOT = pathlib.Path(__file__).resolve().parents[1]
TOPICS_RS_DEFAULT = ROOT / "crates" / "arw-topics" / "src" / "lib.rs"


def github_blob_base() -> str:
    """Return a GitHub blob base URL inferred from the current checkout."""
    env_base = os.getenv("REPO_BLOB_BASE")
    if env_base:
        return env_base.rstrip("/") + "/"
    try:
        remote = subprocess.check_output(
            ["git", "config", "--get", "remote.origin.url"], text=True
        ).strip()
        match = re.search(r"github\\.com[:/]{1,2}([^/]+)/([^/.]+)", remote)
        if match:
            owner, repo = match.group(1), match.group(2)
            return f"https://github.com/{owner}/{repo}/blob/main/"
    except Exception:
        pass
    return "https://github.com/t3hw00t/ARW/blob/main/"


def _stable_now_timestamp(paths: Sequence[pathlib.Path]) -> str:
    """Return a stable ISO8601 timestamp based on the newest commit touching paths."""
    git_args = ["git", "log", "-1", "--format=%cI", "--"] + [str(p) for p in paths if p]
    try:
        ts = subprocess.check_output(git_args, text=True).strip()
        if ts:
            return ts.replace("+00:00", "Z")
    except Exception:
        pass
    env_ts = os.getenv("REPRO_NOW")
    if env_ts:
        return env_ts
    return (
        datetime.datetime.utcnow()
        .replace(tzinfo=datetime.timezone.utc)
        .isoformat(timespec="seconds")
        .replace("+00:00", "Z")
    )


def parse_topics_rs(path: pathlib.Path | None = None, *, include_defaults: Iterable[str] | None = None) -> Set[str]:
    """Parse telemetry topic constants from the arw-topics crate."""
    topic_path = path or TOPICS_RS_DEFAULT
    topics: Set[str] = set(include_defaults or [])
    if not topic_path.exists():
        return topics
    text = topic_path.read_text(encoding="utf-8", errors="ignore")
    topics.update(re.findall(r'pub const [A-Z0-9_]+:\s*&str\s*=\s*"([^"\\]+)";', text))
    return topics


def check_paths_exist(paths: Iterable[str]) -> List[str]:
    """Return the subset of paths (relative to repo root) that are missing."""
    missing: List[str] = []
    for rel in paths:
        if not rel:
            continue
        resolved = ROOT / rel
        if not resolved.exists():
            missing.append(rel)
    return missing


def merge_lists(*lists: Iterable[str] | None) -> List[str]:
    """Merge iterables preserving order while deduplicating entries."""
    seen: Set[str] = set()
    merged: List[str] = []
    for lst in lists:
        if not lst:
            continue
        for item in lst:
            if item not in seen:
                merged.append(item)
                seen.add(item)
    return merged
