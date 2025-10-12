#!/usr/bin/env python3
"""
Generate interfaces/mini_agents.json from catalog/mini_agents/*.yaml manifests.
Requires: PyYAML, jsonschema
"""
from __future__ import annotations

import argparse
import json
import pathlib
import sys
from datetime import datetime, timezone
from typing import Any, Dict, List

import yaml

try:
    import jsonschema
except ImportError as exc:  # pragma: no cover - handled at runtime
    raise SystemExit(
        "error: jsonschema is required (pip install jsonschema>=4.0)"
    ) from exc

from doc_utils import ROOT  # pylint: disable=wrong-import-position

SCHEMA_PATH = ROOT / "spec" / "schemas" / "mini_agent.json"
SOURCE_DIR = ROOT / "catalog" / "mini_agents"
OUTPUT_PATH = ROOT / "interfaces" / "mini_agents.json"


def load_schema() -> Dict[str, Any]:
    with SCHEMA_PATH.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def stable_timestamp(previous: Dict[str, Any] | None, items: List[Dict[str, Any]]) -> str:
    now = (
        datetime.now(timezone.utc)
        .isoformat(timespec="seconds")
        .replace("+00:00", "Z")
    )
    if previous and previous.get("items") == items:
        return previous.get("generated_at", now)
    return now


def discover_manifests() -> List[pathlib.Path]:
    if not SOURCE_DIR.exists():
        return []
    candidates: List[pathlib.Path] = []
    for path in sorted(SOURCE_DIR.iterdir()):
        if path.suffix.lower() in {".yaml", ".yml", ".json"}:
            candidates.append(path)
    return candidates


def load_manifest(path: pathlib.Path) -> Dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        text = handle.read()
    if path.suffix.lower() in {".yaml", ".yml"}:
        data = yaml.safe_load(text)
    else:
        data = json.loads(text)
    if not isinstance(data, dict):
        raise ValueError(f"{path} root must be a mapping")
    return data


def validate_items(items: List[Dict[str, Any]], schema: Dict[str, Any]) -> None:
    validator = jsonschema.Draft7Validator(schema)
    for item in items:
        errors = sorted(validator.iter_errors(item), key=lambda err: err.path)
        if errors:
            messages = "; ".join(
                f"{'/'.join(str(p) for p in err.path)}: {err.message}" for err in errors
            )
            raise ValueError(f"schema validation failed: {messages}")


def build_catalog(items: List[Dict[str, Any]], previous: Dict[str, Any] | None) -> Dict[str, Any]:
    generated_at = stable_timestamp(previous, items)
    catalog = {
        "version": 1,
        "generated_at": generated_at,
        "schema": "spec/schemas/mini_agent.json",
        "items": items,
    }
    return catalog


def write_output(catalog: Dict[str, Any]) -> None:
    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    with OUTPUT_PATH.open("w", encoding="utf-8") as handle:
        json.dump(catalog, handle, indent=2, sort_keys=False)
        handle.write("\n")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Generate interfaces/mini_agents.json from catalog manifests."
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Verify output is up to date without writing.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    schema = load_schema()
    manifests = discover_manifests()
    if not manifests:
        print(f"error: no manifests found under {SOURCE_DIR}", file=sys.stderr)
        return 2

    items: List[Dict[str, Any]] = []
    for path in manifests:
        data = load_manifest(path)
        data.setdefault("status", "beta")
        items.append(data)

    items.sort(key=lambda entry: entry.get("id", ""))
    validate_items(items, schema)

    previous = None
    if OUTPUT_PATH.exists():
        with OUTPUT_PATH.open("r", encoding="utf-8") as handle:
            try:
                previous = json.load(handle)
            except json.JSONDecodeError:
                previous = None

    catalog = build_catalog(items, previous)

    if args.check:
        current = previous or {}
        if current == catalog:
            return 0
        print(
            f"{OUTPUT_PATH.relative_to(ROOT)} is out of date; re-run without --check",
            file=sys.stderr,
        )
        return 1

    write_output(catalog)
    print(
        f"wrote {OUTPUT_PATH.relative_to(ROOT)} ({len(items)} mini-agent entries)",
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
