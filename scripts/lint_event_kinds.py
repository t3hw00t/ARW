#!/usr/bin/env python3
import re, sys, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]

PAT_PUBLISH = re.compile(r"publish\(\s*\"([^\"]+)\"")
PAT_SUBJECT = re.compile(r"arw\.events\.[^\s\"]+")

def is_camel(s: str) -> bool:
    return any(c.isupper() for c in s)

def scan_file(p: pathlib.Path):
    bad = []
    try:
        s = p.read_text(encoding='utf-8', errors='ignore')
    except Exception:
        return bad
    for m in PAT_PUBLISH.finditer(s):
        kind = m.group(1)
        if is_camel(kind):
            bad.append((p, 'publish_kind', kind))
    for m in PAT_SUBJECT.finditer(s):
        subj = m.group(0)
        if is_camel(subj):
            bad.append((p, 'subject', subj))
    return bad

def main():
    roots = [ROOT / 'apps', ROOT / 'crates', ROOT / 'docs', ROOT / 'interfaces']
    bad = []
    for r in roots:
        for p in r.rglob('*'):
            if not p.is_file():
                continue
            if p.suffix in {'.rs', '.html', '.yaml', '.yml', '.json', '.ts', '.js'}:
                bad.extend(scan_file(p))
    if bad:
        for p, kind, val in bad:
            print(f"::error file={p}::{kind} uses CamelCase or uppercase: {val}")
        print(f"Found {len(bad)} CamelCase event kinds/subjects.")
        return 1
    print('No CamelCase event kinds/subjects found.')
    return 0

if __name__ == '__main__':
    sys.exit(main())
