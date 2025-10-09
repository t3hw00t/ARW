#!/usr/bin/env python3
"""Utility to refresh doc metadata headers (Updated: lines) in-place."""

from __future__ import annotations

import argparse
import datetime as _dt
import pathlib as _pl
import re
import sys
from typing import List, Tuple

DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Update or insert `Updated:` metadata lines in markdown docs."
    )
    parser.add_argument(
        "--date",
        type=str,
        help="Explicit YYYY-MM-DD stamp to write (defaults to today's date).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Report files needing changes without writing them. Exit status is 1 when updates are required.",
    )
    parser.add_argument(
        "files",
        metavar="FILE",
        nargs="+",
        type=_pl.Path,
        help="Markdown files to update.",
    )
    return parser.parse_args()


def resolve_date(value: str | None) -> str:
    if value is None:
        return _dt.date.today().isoformat()
    if not DATE_RE.fullmatch(value):
        raise SystemExit(f"error: invalid --date value '{value}' (expected YYYY-MM-DD)")
    return value


def ensure_updated_line(lines: List[str], date: str) -> Tuple[bool, List[str]]:
    limit = min(40, len(lines))
    for idx in range(limit):
        stripped = lines[idx].strip()
        if stripped.lower().startswith("updated:"):
            if stripped == f"Updated: {date}":
                return False, lines
            new_lines = list(lines)
            leading_ws = lines[idx][: len(lines[idx]) - len(lines[idx].lstrip("\t "))]
            new_lines[idx] = f"{leading_ws}Updated: {date}"
            return True, new_lines

    # Prepare insertion point
    type_idx = None
    h1_idx = None
    front_matter_end = None
    for idx in range(limit):
        stripped = lines[idx].strip()
        if idx == 0 and stripped == "---":
            # find end of front matter
            for jdx in range(1, min(len(lines), 200)):
                if lines[jdx].strip() == "---":
                    front_matter_end = jdx
                    break
        if type_idx is None and stripped.startswith("Type:"):
            type_idx = idx
        if h1_idx is None and stripped.startswith("# "):
            h1_idx = idx
        if type_idx is not None and h1_idx is not None:
            break

    insert_idx: int
    if type_idx is not None:
        insert_idx = type_idx
    elif h1_idx is not None:
        insert_idx = h1_idx + 1
        if insert_idx < len(lines) and lines[insert_idx].strip() == "":
            insert_idx += 1
    elif front_matter_end is not None:
        insert_idx = front_matter_end + 1
    else:
        insert_idx = 0

    new_lines = list(lines)
    new_lines.insert(insert_idx, f"Updated: {date}")
    return True, new_lines


def update_file(path: _pl.Path, date: str, dry_run: bool) -> bool:
    if not path.exists():
        raise SystemExit(f"error: file not found: {path}")
    original = path.read_text(encoding="utf-8").splitlines()
    changed, new_lines = ensure_updated_line(original, date)
    if not changed:
        if dry_run:
            print(f"{path}: up-to-date")
        return False

    if dry_run:
        print(f"{path}: would set Updated: {date}")
    else:
        path.write_text("\n".join(new_lines) + "\n", encoding="utf-8")
        print(f"{path}: set Updated: {date}")
    return True


def main() -> int:
    args = parse_args()
    date_value = resolve_date(args.date)
    changed_any = False
    for file_path in args.files:
        changed = update_file(file_path, date_value, args.dry_run)
        changed_any = changed_any or changed
    if args.dry_run and changed_any:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
