#!/usr/bin/env python3
"""
Convert a YAML file to pretty-printed JSON.

Usage: python3 scripts/yaml_to_json.py <input.yaml> <output.json>
"""
import json
import sys
from pathlib import Path

try:
    import yaml  # type: ignore
except Exception as exc:
    print("error: PyYAML is required (pip install pyyaml)", file=sys.stderr)
    sys.exit(2)


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: yaml_to_json.py <input.yaml> <output.json>", file=sys.stderr)
        return 2
    src = Path(sys.argv[1])
    dst = Path(sys.argv[2])
    data = yaml.safe_load(src.read_text(encoding="utf-8"))
    dst.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

