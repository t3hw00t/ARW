#!/usr/bin/env python3
"""
Generate Interface Release Notes by diffing current specs against a base ref.

Outputs: docs/reference/interface-release-notes.md

Prefers industry tools when available:
 - OpenAPI: tufin/oasdiff (Docker) -> Markdown
 - AsyncAPI: @asyncapi/diff (Node) -> Markdown

Fallback: include simple file-level diff if tools unavailable.

Usage:
  BASE_REF=origin/main python scripts/generate_interface_release_notes.py
"""
import os
import sys
import subprocess as sp
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
OPENAPI = REPO / 'spec' / 'openapi.yaml'
ASYNCAPI = REPO / 'spec' / 'asyncapi.yaml'
OUT = REPO / 'docs' / 'reference' / 'interface-release-notes.md'


def run(cmd, **kw):
    return sp.run(cmd, stdout=sp.PIPE, stderr=sp.PIPE, text=True, **kw)


def have(cmd):
    return run(['bash', '-lc', f'command -v {cmd} >/dev/null 2>&1 && echo 1 || echo 0']).stdout.strip() == '1'


def git_show(refpath: str) -> str:
    p = run(['git', 'show', refpath], cwd=REPO)
    if p.returncode != 0:
        raise RuntimeError(f'git show failed: {refpath}: {p.stderr.strip()}')
    return p.stdout


def gen_openapi_diff(base_ref: str, tmpdir: Path) -> str:
    base = tmpdir / 'openapi.base.yaml'
    head = tmpdir / 'openapi.head.yaml'
    try:
        base.write_text(git_show(f'{base_ref}:spec/openapi.yaml'))
    except Exception:
        return '_OpenAPI base not found; skipping._\n'
    head.write_text(OPENAPI.read_text())
    if have('docker'):
        # Newer oasdiff uses subcommands; prefer 'diff -f markdown'.
        p = run(['bash', '-lc', f'docker run --rm -v {tmpdir}:/tmp -w /tmp tufin/oasdiff:latest diff -f markdown /tmp/{base.name} /tmp/{head.name}'], cwd=REPO)
        if p.returncode == 0 and p.stdout.strip():
            return p.stdout
    # fallback: unified diff
    p = run(['bash', '-lc', f'diff -u {base} {head} || true'])
    return '```diff\n' + p.stdout + '\n```\n'


def gen_asyncapi_diff(base_ref: str, tmpdir: Path) -> str:
    base = tmpdir / 'asyncapi.base.yaml'
    head = tmpdir / 'asyncapi.head.yaml'
    try:
        base.write_text(git_show(f'{base_ref}:spec/asyncapi.yaml'))
    except Exception:
        return '_AsyncAPI base not found; skipping._\n'
    head.write_text(ASYNCAPI.read_text())
    if have('npx'):
        p = run(['bash', '-lc', f'npx --yes @asyncapi/diff {base} {head} --markdown'], cwd=REPO)
        if p.returncode == 0 and p.stdout.strip():
            return p.stdout
    # fallback: unified diff
    p = run(['bash', '-lc', f'diff -u {base} {head} || true'])
    return '```diff\n' + p.stdout + '\n```\n'


def main():
    base_ref = os.environ.get('BASE_REF', 'origin/main')
    with tempfile.TemporaryDirectory() as td:
        tdp = Path(td)
        oa = gen_openapi_diff(base_ref, tdp)
        aa = gen_asyncapi_diff(base_ref, tdp)
    lines = []
    lines.append('---')
    lines.append('title: Interface Release Notes')
    lines.append('---')
    lines.append('')
    lines.append('# Interface Release Notes')
    lines.append('')
    # Avoid per-commit churn by omitting dynamic HEAD rev
    lines.append(f'Base: `{base_ref}`')
    lines.append('')
    lines.append('## OpenAPI (REST)')
    lines.append('')
    lines.append(oa.rstrip())
    lines.append('')
    lines.append('## AsyncAPI (Events)')
    lines.append('')
    lines.append(aa.rstrip())
    lines.append('')
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text('\n'.join(lines) + '\n', encoding='utf-8')
    print(f'wrote {OUT.relative_to(REPO)}')


if __name__ == '__main__':
    sys.exit(main())
