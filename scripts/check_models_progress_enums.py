#!/usr/bin/env python3
"""
Verify that spec/asyncapi.yaml ModelsDownloadProgress status/code enums
match the set of values emitted by the unified server in
apps/arw-server/src/models.rs (publish_progress + DownloadOutcome::Failed).
Exits non-zero if mismatched.
"""
from __future__ import annotations
import re
import sys
from pathlib import Path

import yaml

REPO = Path(__file__).resolve().parents[1]
RUST = REPO / 'apps' / 'arw-server' / 'src' / 'models.rs'
ASYNCAPI = REPO / 'spec' / 'asyncapi.yaml'

STATUS_RE = re.compile(r"publish_progress\([^,]+,\s*Some\(\"([^\"]+)\"\)", re.S)
FAILED_CODE_RE = re.compile(r"DownloadOutcome::Failed\s*\{[^}]*code:\s*\"([^\"]+)\"", re.S)
PROGRESS_CODE_RE = re.compile(r"publish_progress\([^,]+,\s*Some\(\"[^\"]+\"\),\s*Some\(\"([^\"]+)\"\)")


def parse_statuses(text: str) -> list[str]:
    return sorted(set(STATUS_RE.findall(text)))


def parse_codes(text: str) -> list[str]:
    return sorted(set(FAILED_CODE_RE.findall(text)))


def main() -> int:
    rs = RUST.read_text(encoding='utf-8')
    status = parse_statuses(rs)
    codes = set(parse_codes(rs))
    codes.update(PROGRESS_CODE_RE.findall(rs))
    if '"request-timeout"' in rs:
        codes.add('request-timeout')
    codes = sorted(codes)

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
