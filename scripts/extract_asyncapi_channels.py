#!/usr/bin/env python3
"""
Extract AsyncAPI channel keys from spec/asyncapi.yaml into a simple JSON blob
that Spectral can lint reliably without key-selector quirks.

Usage:
  python3 scripts/extract_asyncapi_channels.py [OUT_JSON]

Writes a JSON object: {"channels": ["a.b", "c.d.e", ...]}
"""
import json
import os
import sys

try:
    import yaml  # type: ignore
except Exception as e:  # pragma: no cover
    print(f"error: PyYAML not installed: {e}", file=sys.stderr)
    sys.exit(2)

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))
SPEC = os.path.join(ROOT, "spec", "asyncapi.yaml")


def main() -> int:
    out_path = sys.argv[1] if len(sys.argv) > 1 else "-"
    try:
        with open(SPEC, "r", encoding="utf-8") as f:
            doc = yaml.safe_load(f)
    except FileNotFoundError:
        print(f"error: spec not found: {SPEC}", file=sys.stderr)
        return 2
    except Exception as e:
        print(f"error: failed to parse {SPEC}: {e}", file=sys.stderr)
        return 2

    chans = []
    ch = (doc or {}).get("channels") or {}
    if isinstance(ch, dict):
        chans = list(ch.keys())
    payload = {"channels": sorted(chans)}
    data = json.dumps(payload, ensure_ascii=False, indent=2)
    if out_path == "-":
        print(data)
    else:
        with open(out_path, "w", encoding="utf-8") as f:
            f.write(data)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

