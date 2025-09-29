#!/usr/bin/env python3
"""Validate that spec/openapi.yaml matches curated summaries/descriptions."""
from __future__ import annotations

from pathlib import Path
import sys
import yaml

REPO = Path(__file__).resolve().parent.parent
SPEC_PATH = REPO / "spec" / "openapi.yaml"
CURATED_PATH = REPO / "spec" / "operation_docs.yaml"

METHODS = {"get", "post", "put", "delete", "patch", "head", "options", "trace"}
FIELDS = ("summary", "description")


def load_yaml(path: Path):
    if not path.exists():
        raise SystemExit(f"missing required file: {path}")
    with path.open("r", encoding="utf-8") as handle:
        return yaml.safe_load(handle)


def collect_ops(doc: dict[str, object]):
    paths = doc.get("paths") or {}
    for path, raw_methods in paths.items():
        if not isinstance(raw_methods, dict):
            continue
        for method, op in raw_methods.items():
            method_lower = str(method).lower()
            if method_lower not in METHODS:
                continue
            yield path, method_lower, op or {}


def main() -> int:
    spec = load_yaml(SPEC_PATH)
    curated = load_yaml(CURATED_PATH) or {}

    spec_index = {(path, method): op for path, method, op in collect_ops(spec)}
    curated_index: set[tuple[str, str]] = set()

    errors: list[str] = []

    for path, methods in curated.items():
        if not isinstance(methods, dict):
            errors.append(f"{path}: expected mapping of methods, found {type(methods).__name__}")
            continue
        for method, fields in methods.items():
            method_lower = str(method).lower()
            if method_lower not in METHODS:
                errors.append(f"{path} {method}: unsupported HTTP method")
                continue
            spec_op = spec_index.get((path, method_lower))
            if spec_op is None:
                errors.append(f"{path} {method_upper(method_lower)} missing from spec")
                continue
            if not isinstance(fields, dict):
                errors.append(f"{path} {method_upper(method_lower)}: expected mapping for curated fields")
                continue
            curated_index.add((path, method_lower))
            for field in FIELDS:
                if field not in fields:
                    errors.append(f"{path} {method_upper(method_lower)} missing '{field}' in curated docs")
                    continue
                curated_value = fields.get(field)
                if not isinstance(curated_value, str) or not curated_value.strip():
                    errors.append(
                        f"{path} {method_upper(method_lower)} {field} must be a non-empty string"
                    )
                    continue
                spec_value = spec_op.get(field)
                if spec_value != curated_value:
                    errors.append(
                        f"{path} {method_upper(method_lower)} {field} mismatch\n"
                        f"  curated: {curated_value!r}\n"
                        f"  spec:    {spec_value!r}"
                    )
    missing_curated = sorted(set(spec_index.keys()) - curated_index)
    for path, method in missing_curated:
        errors.append(f"{path} {method_upper(method)} missing from curated docs")
    if errors:
        print("Operation doc drift detected:")
        for entry in errors:
            print(f"- {entry}")
        print("Run 'python3 scripts/apply_operation_docs.py' to refresh the spec.")
        return 1
    return 0


def method_upper(name: str) -> str:
    return name.upper()


if __name__ == "__main__":
    sys.exit(main())
