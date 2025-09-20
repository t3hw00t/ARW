#!/usr/bin/env python3
"""
Generate docs/reference/deprecations.md from spec/openapi.yaml and descriptors.
Requires: PyYAML
"""
import os
import sys
import yaml
from datetime import datetime, timezone
import hashlib

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPEC = os.path.join(REPO, 'spec', 'openapi.yaml')
DESC = os.path.join(REPO, 'interfaces', 'http', 'arw-server', 'descriptor.yaml')
OUT = os.path.join(REPO, 'docs', 'reference', 'deprecations.md')


def file_sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        for chunk in iter(lambda: f.read(8192), b''):
            h.update(chunk)
    return h.hexdigest()


def load_yaml(path):
    with open(path, 'r', encoding='utf-8') as f:
        return yaml.safe_load(f)


def first(s):
    return (s or [None])[0]


def gen():
    spec = load_yaml(SPEC)
    desc = None
    try:
        desc = load_yaml(DESC)
    except Exception:
        desc = None
    default_sunset = (desc or {}).get('sunset')
    rows = []
    for path, ops in (spec.get('paths') or {}).items():
        if not isinstance(ops, dict):
            continue
        for method, op in ops.items():
            if not isinstance(op, dict):
                continue
            mlow = str(method).lower()
            if mlow not in ('get','post','put','delete','patch','head','options','trace'):
                continue
            if not op.get('deprecated'):
                continue
            sunset = op.get('x-sunset') or default_sunset or ''
            tags = op.get('tags') or []
            row = {
                'method': mlow.upper(),
                'path': path,
                'tag': first(tags) or '',
                'sunset': str(sunset),
                'summary': op.get('summary') or '',
            }
            rows.append(row)
    # sort by sunset then path
    def key(r):
        s = r['sunset'] or '9999-12-31T23:59:59Z'
        return (s, r['path'], r['method'])
    rows.sort(key=key)

    lines = []
    lines.append('---')
    lines.append('title: Interface Deprecations')
    lines.append('---')
    lines.append('')
    lines.append('# Interface Deprecations')
    lines.append('')
    # Stable header: embed spec content hash to avoid timestamp churn
    try:
        sh = file_sha256(SPEC)[:12]
        hdr = f'_Generated from spec/openapi.yaml (sha256:{sh}). Do not edit._'
    except Exception:
        # Fallback: ISO now, but this path should rarely execute
        hdr = datetime.now(timezone.utc).isoformat(timespec='seconds').replace('+00:00', 'Z')
        hdr = f'_Generated {hdr} from spec/openapi.yaml. Do not edit._'
    lines.append(hdr)
    lines.append('')
    lines.append('When an operation is marked deprecated, the runtime emits standard headers (Deprecation, optionally Sunset and Link rel="deprecation").')
    lines.append('')
    if not rows:
        lines.append('No deprecated endpoints.')
    else:
        lines.append('| Method | Path | Tag | Sunset | Summary |')
        lines.append('|---|---|---|---|---|')
        for r in rows:
            lines.append(f"| {r['method']} | `{r['path']}` | {r['tag']} | {r['sunset']} | {r['summary'].replace('|','\\|')} |")
    out = '\n'.join(lines) + '\n'
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, 'w', encoding='utf-8') as f:
        f.write(out)
    print(f'wrote {os.path.relpath(OUT, REPO)} ({len(rows)} items)')


if __name__ == '__main__':
    sys.exit(gen())
