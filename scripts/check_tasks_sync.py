#!/usr/bin/env python3
"""Ensure docs/developer/tasks.md matches .arw/tasks.json."""
from __future__ import annotations

import difflib
import json
import sys
from collections import defaultdict
from pathlib import Path

from docgen_core import GEN_COMMENT, TASKS_JSON, TASKS_MD, format_task_entry


def normalize_newlines(text: str) -> str:
    return text.replace("\r\n", "\n")


def load_tasks(path: Path) -> dict:
    if not path.exists():
        raise SystemExit(f"[tasks-check] {path} is missing; run scripts/tasks_sync.py")
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise SystemExit(f"[tasks-check] failed to parse {path}: {exc}") from exc


def extract_updated_label(doc_text: str) -> str:
    for line in doc_text.splitlines():
        if line.startswith("Updated: "):
            return line.split("Updated: ", 1)[1].strip()
    raise SystemExit("[tasks-check] docs/developer/tasks.md lacks an 'Updated:' line")


def render_markdown(tasks: list[dict], updated_label: str) -> str:
    grouped: dict[str, list[dict]] = defaultdict(list)
    for task in tasks:
        status = (task.get("status") or "todo").lower()
        grouped[status].append(task)
    for items in grouped.values():
        items.sort(key=lambda entry: entry.get("updated") or "", reverse=True)

    sections = [
        ("todo", "To Do"),
        ("in_progress", "In Progress"),
        ("paused", "Paused"),
        ("done", "Done"),
    ]

    lines: list[str] = [
        "---",
        "title: Tasks Status",
        "---",
        "",
        "# Tasks Status",
        "",
        GEN_COMMENT,
        "",
        f"Updated: {updated_label}",
        "",
    ]

    for status_key, title in sections:
        lines.append(f"## {title}")
        entries = grouped.get(status_key, [])
        if entries:
            lines.extend(format_task_entry(entry) for entry in entries)
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def main() -> int:
    tasks_data = load_tasks(TASKS_JSON)
    tasks = tasks_data.get("tasks")
    if not isinstance(tasks, list):
        raise SystemExit("[tasks-check] .arw/tasks.json does not contain a task list")

    doc_text = normalize_newlines(TASKS_MD.read_text(encoding="utf-8"))
    updated_label = extract_updated_label(doc_text)
    expected = render_markdown(tasks, updated_label)

    if doc_text != expected:
        diff = "".join(
            difflib.unified_diff(
                expected.splitlines(True),
                doc_text.splitlines(True),
                fromfile="expected",
                tofile=str(TASKS_MD),
            )
        )
        sys.stderr.write("[tasks-check] docs/developer/tasks.md is out of sync with .arw/tasks.json\n")
        sys.stderr.write(diff)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
