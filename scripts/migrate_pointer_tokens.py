#!/usr/bin/env python3
"""
Canonicalise legacy pointer tokens and ensure consent metadata exists.

This utility walks an ARW state directory, fixes pointer casing/normalisation in
SQLite stores (memory records) and JSON blobs, and optionally writes the
updates in place.  Run with `--dry-run` first to review changes.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sqlite3
import sys
import unicodedata
from pathlib import Path
from typing import Any, Iterable, Tuple

POINTER_RE = re.compile(r"<@[^>\s]{1,255}>")


def canonicalise_pointer(token: str) -> Tuple[str, bool]:
    """Normalise a pointer token (NFKC, lowercase prefix)."""
    normalised = unicodedata.normalize("NFKC", token).replace("\r\n", "\n")
    if len(normalised) > 256 or not normalised.startswith("<@") or not normalised.endswith(">"):
        return token, False

    try:
        prefix, remainder = normalised[2:-1].split(":", 1)
    except ValueError:
        return token, False

    prefix_lower = prefix.lower()
    canonical = f"<@{prefix_lower}:{remainder}>"
    return canonical, canonical != token


def canonicalise_text(value: str) -> Tuple[str, bool]:
    """Canonicalise pointer tokens inside a plain text blob."""
    changed = False
    parts = []
    last = 0
    for match in POINTER_RE.finditer(value):
        start, end = match.span()
        parts.append(value[last:start])
        replacement, updated = canonicalise_pointer(match.group(0))
        parts.append(replacement)
        if updated:
            changed = True
        last = end
    if not parts:
        return value, False
    parts.append(value[last:])
    return "".join(parts), changed


def canonicalise_json(value: Any, default_consent: str) -> Tuple[Any, bool]:
    """Recursively canonicalise pointer tokens in JSON-compatible structures."""
    changed = False
    if isinstance(value, dict):
        for key, item in list(value.items()):
            if isinstance(item, str):
                updated, token_changed = canonicalise_pointer(item)
                if token_changed:
                    value[key] = updated
                    changed = True
                    if key == "pointer" and "consent" not in value:
                        value["consent"] = default_consent
                        changed = True
                else:
                    canonical_text, text_changed = canonicalise_text(item)
                    if text_changed:
                        value[key] = canonical_text
                        changed = True
            else:
                new_item, child_changed = canonicalise_json(item, default_consent)
                if child_changed:
                    value[key] = new_item
                    changed = True
        # Normalise domain casing when present.
        domain = value.get("domain")
        if isinstance(domain, str):
            lowered = domain.lower()
            if lowered != domain:
                value["domain"] = lowered
                changed = True
        return value, changed
    if isinstance(value, list):
        for idx, item in enumerate(value):
            new_item, item_changed = canonicalise_json(item, default_consent)
            if item_changed:
                value[idx] = new_item
                changed = True
        return value, changed
    if isinstance(value, str):
        canonical_str, text_changed = canonicalise_text(value)
        return canonical_str, text_changed
    return value, False


def process_json_text(text: str, default_consent: str) -> Tuple[str, bool]:
    """Attempt to canonicalise JSON text."""
    try:
        payload = json.loads(text)
    except json.JSONDecodeError:
        return canonicalise_text(text)

    payload, changed = canonicalise_json(payload, default_consent)
    if changed:
        return json.dumps(payload, ensure_ascii=False, sort_keys=True), True
    return text, False


def update_memory_records(db_path: Path, dry_run: bool, default_consent: str) -> int:
    """Canonicalise pointer tokens in memory_records table for a given SQLite DB."""
    conn = sqlite3.connect(f"file:{db_path}?mode=rw", uri=True)
    conn.row_factory = sqlite3.Row
    cur = conn.cursor()

    cur.execute(
        "SELECT name FROM sqlite_master WHERE type='table' AND name='memory_records'"
    )
    if not cur.fetchone():
        return 0

    cur.execute("SELECT id, value, extra, links, source FROM memory_records")
    updates = []
    for row in cur.fetchall():
        updated_fields = {}
        for column in ("value", "extra", "links", "source"):
            raw = row[column]
            if raw is None:
                continue
            new_text, changed = process_json_text(raw, default_consent)
            if changed:
                updated_fields[column] = new_text
        if updated_fields:
            updates.append((row["id"], updated_fields))

    if not updates or dry_run:
        return len(updates)

    for record_id, fields in updates:
        assignments = ", ".join(f"{col}=?" for col in fields)
        params = list(fields.values())
        params.append(record_id)
        cur.execute(
            f"UPDATE memory_records SET {assignments}, updated=datetime('now') WHERE id=?",
            params,
        )
    conn.commit()
    return len(updates)


def iter_sqlite_files(state_dir: Path) -> Iterable[Path]:
    for root, _, files in os.walk(state_dir):
        for filename in files:
            if filename.endswith(".sqlite"):
                yield Path(root) / filename


def canonicalise_json_files(paths: Iterable[Path], dry_run: bool, default_consent: str) -> int:
    changed = 0
    for path in paths:
        original = path.read_text(encoding="utf-8")
        updated, modified = process_json_text(original, default_consent)
        if modified:
            changed += 1
            if not dry_run:
                path.write_text(updated + ("\n" if not updated.endswith("\n") else ""), encoding="utf-8")
    return changed


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Canonicalise pointer tokens in ARW state and config files."
    )
    parser.add_argument(
        "--state-dir",
        type=Path,
        default=None,
        help="Path to the ARW state directory (defaults to ARW_STATE_DIR or ./state).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Analyse and report changes without writing them.",
    )
    parser.add_argument(
        "--default-consent",
        default="private",
        choices=["private", "shared", "public"],
        help="Consent level to inject when pointer records lack explicit consent.",
    )
    parser.add_argument(
        "--extra-json",
        nargs="*",
        type=Path,
        default=[],
        help="Additional JSON files to canonicalise (docs, configs, etc.).",
    )
    args = parser.parse_args(argv)

    state_dir = args.state_dir
    if state_dir is None:
        env_state = os.environ.get("ARW_STATE_DIR")
        state_dir = Path(env_state) if env_state else Path("state")

    if not state_dir.exists():
        print(f"[warn] state directory {state_dir} does not exist", file=sys.stderr)

    total_updates = 0
    for sqlite_path in iter_sqlite_files(state_dir):
        try:
            updated_rows = update_memory_records(sqlite_path, args.dry_run, args.default_consent)
        except sqlite3.Error as exc:
            print(f"[warn] skipping {sqlite_path}: {exc}", file=sys.stderr)
            continue
        if updated_rows:
            mode = "would update" if args.dry_run else "updated"
            print(f"[info] {mode} {updated_rows} memory record(s) in {sqlite_path}")
            total_updates += updated_rows

    json_targets = list(args.extra_json)
    config_dir = state_dir / "config"
    if config_dir.exists():
        json_targets.extend(config_dir.glob("**/*.json"))

    json_changed = canonicalise_json_files(json_targets, args.dry_run, args.default_consent)
    if json_changed:
        mode = "would update" if args.dry_run else "updated"
        print(f"[info] {mode} {json_changed} JSON file(s)")

    if args.dry_run:
        print(f"[summary] dry-run complete; {total_updates} record(s) require updates")
    else:
        print(f"[summary] canonicalised {total_updates} record(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
