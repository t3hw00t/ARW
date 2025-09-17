#!/usr/bin/env python3
"""
Convert spec/openapi.yaml to docs/static/openapi.json (pretty JSON).
Requires: PyYAML
"""
import json
import os
import sys
try:
    import yaml
except ModuleNotFoundError:  # pragma: no cover - optional dependency
    yaml = None

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(REPO, 'spec', 'openapi.yaml')
DST = os.path.join(REPO, 'docs', 'static', 'openapi.json')


def main():
    if yaml is None:
        print("warning: PyYAML not installed; skipping openapi.json generation", file=sys.stderr)
        return 0
    if not os.path.exists(SRC):
        print("warning: spec/openapi.yaml missing; skipping openapi.json generation", file=sys.stderr)
        return 0
    with open(SRC, 'r', encoding='utf-8') as f:
        y = yaml.safe_load(f)
    os.makedirs(os.path.dirname(DST), exist_ok=True)
    with open(DST, 'w', encoding='utf-8') as f:
        json.dump(y, f, ensure_ascii=False, indent=2)
        f.write('\n')
    print(f'wrote {os.path.relpath(DST, REPO)}')


if __name__ == '__main__':
    sys.exit(main())

