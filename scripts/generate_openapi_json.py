#!/usr/bin/env python3
"""
Convert spec/openapi.yaml to docs/static/openapi.json (pretty JSON).
Requires: PyYAML
"""
import json
import os
import sys
import yaml

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(REPO, 'spec', 'openapi.yaml')
DST = os.path.join(REPO, 'docs', 'static', 'openapi.json')


def main():
    with open(SRC, 'r', encoding='utf-8') as f:
        y = yaml.safe_load(f)
    os.makedirs(os.path.dirname(DST), exist_ok=True)
    with open(DST, 'w', encoding='utf-8') as f:
        json.dump(y, f, ensure_ascii=False, indent=2)
        f.write('\n')
    print(f'wrote {os.path.relpath(DST, REPO)}')


if __name__ == '__main__':
    sys.exit(main())

