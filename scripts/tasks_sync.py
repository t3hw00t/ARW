#!/usr/bin/env python3
"""Regenerate developer task documentation from `.arw/tasks.json`."""
from __future__ import annotations

import argparse
import sys

try:
    from docgen_core import Runner, update_tasks_docs
except ImportError as exc:  # pragma: no cover - defensive guard
    raise SystemExit(f"[tasks-sync] unable to import docgen_core: {exc}") from exc


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Regenerate docs/developer/tasks.md from .arw/tasks.json.",
    )
    parser.add_argument(
        "--quiet",
        action="store_true",
        help="Suppress informational logs.",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    runner = Runner(quiet=args.quiet)
    update_tasks_docs(runner)

    if runner.failures:
        for msg in runner.failures:
            print(f"[tasks-sync] {msg}", file=sys.stderr)
        return 1
    if runner.warnings and not runner.quiet:
        for msg in runner.warnings:
            print(f"[tasks-sync] warning: {msg}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
