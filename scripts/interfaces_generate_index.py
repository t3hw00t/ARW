#!/usr/bin/env python3
"""
Generate interfaces/index.yaml by scanning interfaces/*/*/descriptor.yaml files.
Requires: PyYAML (pip install pyyaml)
"""
import os
import sys
import yaml
from datetime import datetime, timezone

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
INTERFACES_DIR = os.path.join(REPO_ROOT, 'interfaces')
INDEX_PATH = os.path.join(INTERFACES_DIR, 'index.yaml')


def find_descriptors(root: str):
    for dirpath, dirnames, filenames in os.walk(root):
        if 'descriptor.yaml' in filenames:
            yield os.path.join(dirpath, 'descriptor.yaml')


def rel_path(path):
    return os.path.relpath(path, REPO_ROOT).replace('\\', '/')


def main():
    descriptors = list(find_descriptors(INTERFACES_DIR))
    items = []
    for d in sorted(descriptors):
        try:
            with open(d, 'r', encoding='utf-8') as f:
                y = yaml.safe_load(f)
            iid = y.get('id')
            kind = y.get('kind')
            if not iid or not kind:
                print(f"warn: skipping {d} (missing id/kind)", file=sys.stderr)
                continue
            path = rel_path(os.path.dirname(d))
            items.append({ 'id': iid, 'kind': kind, 'path': path })
        except Exception as e:
            print(f"warn: failed to parse {d}: {e}", file=sys.stderr)
    now = datetime.now(timezone.utc).isoformat(timespec='seconds').replace('+00:00', 'Z')
    prev = None
    if os.path.exists(INDEX_PATH):
        try:
            prev = yaml.safe_load(open(INDEX_PATH, 'r', encoding='utf-8'))
        except Exception:
            prev = None
    gen_at = now
    if prev and isinstance(prev, dict) and prev.get('interfaces') == items:
        gen_at = prev.get('generated_at') or now
    idx = {
        'version': 1,
        'generated_at': gen_at,
        'interfaces': items,
    }
    os.makedirs(os.path.dirname(INDEX_PATH), exist_ok=True)
    with open(INDEX_PATH, 'w', encoding='utf-8') as f:
        yaml.safe_dump(idx, f, sort_keys=False)
    print(f"wrote {rel_path(INDEX_PATH)} ({len(items)} interfaces)")


if __name__ == '__main__':
    sys.exit(main())
