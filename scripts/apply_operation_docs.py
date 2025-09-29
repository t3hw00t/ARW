#!/usr/bin/env python3
"""Apply curated operation summaries/descriptions to spec/openapi.yaml."""
import os
from pathlib import Path

import yaml

REPO = Path(__file__).resolve().parent.parent
SPEC = REPO / "spec" / "openapi.yaml"
CURATED = REPO / "spec" / "operation_docs.yaml"


def load_yaml(path: Path):
    with path.open("r", encoding="utf-8") as handle:
        return yaml.safe_load(handle)


def save_yaml(path: Path, data):
    with path.open("w", encoding="utf-8") as handle:
        yaml.safe_dump(data, handle, sort_keys=False)


def ensure_operation(doc, path, method):
    try:
        op = doc["paths"][path][method]
    except KeyError as exc:
        raise KeyError(f"Missing {method.upper()} {path} in OpenAPI spec") from exc
    if not isinstance(op, dict):
        raise TypeError(f"Operation {method.upper()} {path} is not a mapping")
    return op


def apply_curated_docs():
    spec = load_yaml(SPEC)
    curated = load_yaml(CURATED)
    if curated is None:
        print("operation_docs.yaml is empty; nothing to do")
        return 0

    changed = False
    for path, methods in curated.items():
        if not isinstance(methods, dict):
            raise TypeError(f"Expected mapping for path {path}")
        for method, fields in methods.items():
            method_key = method.lower()
            op = ensure_operation(spec, path, method_key)
            if not isinstance(fields, dict):
                raise TypeError(f"Expected mapping for {method.upper()} {path}")
            for key in ("summary", "description"):
                if key in fields:
                    value = fields[key]
                    if value is None:
                        continue
                    if op.get(key) != value:
                        op[key] = value
                        changed = True
    if changed:
        save_yaml(SPEC, spec)
        print("updated spec/openapi.yaml")
        return 1
    print("no changes")
    return 0


if __name__ == "__main__":
    raise SystemExit(apply_curated_docs())
