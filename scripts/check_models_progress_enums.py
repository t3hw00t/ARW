#!/usr/bin/env python3
"""
Verify that spec/asyncapi.yaml ModelsDownloadProgress status/code enums
match the single source of truth in
apps/arw-svc/src/resources/models_service.rs (PROGRESS_STATUS/CODES).
Exits non-zero if mismatched.
"""
from __future__ import annotations
import re
import sys
from pathlib import Path

import yaml

REPO = Path(__file__).resolve().parents[1]
RUST = REPO / 'apps' / 'arw-svc' / 'src' / 'resources' / 'models_service.rs'
ASYNCAPI = REPO / 'spec' / 'asyncapi.yaml'

def parse_enum(name: str, text: str) -> list[str]:
    # e.g., pub const PROGRESS_STATUS: [&'static str; N] = [ "a", "b", ];
    m = re.search(rf"{name}\s*:\s*\[\s*&'static\s+str[^\]]*\]\s*=\s*\[(.*?)\];",
                  text, re.S)
    if not m:
        raise RuntimeError(f"array {name} not found")
    inner = m.group(1)
    items = []
    for s in re.findall(r'"([^"]+)"', inner):
        items.append(s)
    return items

def main() -> int:
    rs = RUST.read_text(encoding='utf-8')
    status = parse_enum('PROGRESS_STATUS', rs)
    codes = parse_enum('PROGRESS_CODES', rs)

    doc = yaml.safe_load(ASYNCAPI.read_text(encoding='utf-8'))
    msg = doc['components']['messages']['ModelsDownloadProgress']
    pen = msg['payload']['properties']
    s_en = pen['status']['enum']
    c_en = pen['code']['enum']

    s_miss = sorted(set(status) ^ set(s_en))
    c_miss = sorted(set(codes) ^ set(c_en))
    ok = True
    if s_miss:
        print('status enum mismatch:', s_miss)
        ok = False
    if c_miss:
        print('code enum mismatch:', c_miss)
        ok = False
    if not ok:
        print('\nTip: update spec/asyncapi.yaml to match ModelsService enums.')
        return 2
    print('Enums OK (status & code match).')
    return 0

if __name__ == '__main__':
    sys.exit(main())

