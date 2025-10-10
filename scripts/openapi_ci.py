#!/usr/bin/env python3
"""Utility helpers for OpenAPI CI workflows."""

from __future__ import annotations

import argparse
import pathlib
from typing import Any

import yaml


def _strip_deprecated(node: Any) -> Any:
    if isinstance(node, dict):
        return {
            key: _strip_deprecated(value)
            for key, value in node.items()
            if key != "deprecated"
        }
    if isinstance(node, list):
        return [_strip_deprecated(item) for item in node]
    return node


def normalize(args: argparse.Namespace) -> None:
    source = pathlib.Path(args.input)
    target = pathlib.Path(args.output)
    data = yaml.safe_load(source.read_text(encoding="utf-8"))
    if args.strip_deprecated:
        data = _strip_deprecated(data)
    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open("w", encoding="utf-8") as fh:
        yaml.safe_dump(data, fh, sort_keys=args.sort_keys)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Helpers for CI OpenAPI sync checks."
    )
    sub = parser.add_subparsers(dest="command", required=True)

    norm = sub.add_parser("normalize", help="Write a normalized copy of a spec.")
    norm.add_argument("input", help="Path to source OpenAPI/AsyncAPI document.")
    norm.add_argument("output", help="Destination path for normalized YAML.")
    norm.add_argument(
        "--strip-deprecated",
        action="store_true",
        help="Remove `deprecated` keys before emitting.",
    )
    norm.add_argument(
        "--sort-keys",
        action="store_true",
        help="Sort keys when serializing (deterministic diff).",
    )
    norm.set_defaults(func=normalize)

    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
