#!/usr/bin/env python3
import re, sys, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]

PAT_PUBLISH = re.compile(r"publish\(\s*\"([^\"]+)\"")
# Strict check: disallow string-literal publish on service bus (enforce constants)
PAT_PUBLISH_STRING = re.compile(r"\b\w+\.publish\(\s*\"([^\"]+)\"")
PAT_SUBJECT = re.compile(r"arw\.events\.[^\s\"]+")
PAT_TOPIC_CONST = re.compile(r"pub const TOPIC_[A-Z0-9_]+: &str = \"([^\"]+)\"")

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
    # Enforce dot.case in topic constants
    if p.name == 'topics.rs' and 'apps/arw-svc/src/ext/topics.rs' in str(p):
        for m in PAT_TOPIC_CONST.finditer(s):
            val = m.group(1)
            if not re.fullmatch(r"[a-z0-9]+(\.[a-z0-9]+)*", val or ""):
                bad.append((p, 'topic_constant_not_dot_case', val))
    # Enforce constants in service (apps/arw-svc) for Bus.publish calls
    if p.suffix == '.rs' and 'apps/arw-svc/src/' in str(p):
        for m in PAT_PUBLISH_STRING.finditer(s):
            lit = m.group(1)
            bad.append((p, 'publish_string_literal', lit))
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
