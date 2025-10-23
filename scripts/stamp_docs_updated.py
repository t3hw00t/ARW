#!/usr/bin/env python3
import subprocess, sys, os, datetime, re

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
DOCS = os.path.join(ROOT, 'docs')

def git_status_has_changes(path: str) -> bool:
    rel = os.path.relpath(path, ROOT).replace(os.sep, '/')
    try:
        out = subprocess.check_output(
            ['git','status','--porcelain','--', rel],
            cwd=ROOT, stderr=subprocess.DEVNULL, text=True
        ).strip()
        return bool(out)
    except Exception:
        return False

def git_last_date(path: str) -> str:
    if git_status_has_changes(path):
        return datetime.date.today().isoformat()
    try:
        out = subprocess.check_output(
            ['git','log','-1','--format=%ad','--date=format:%Y-%m-%d','--', path],
            cwd=ROOT, stderr=subprocess.DEVNULL, text=True
        ).strip()
        if out:
            return out
    except Exception:
        pass
    return datetime.date.today().isoformat()

def has_updated_or_generated(lines):
    for i, line in enumerate(lines[:40]):
        if line.startswith('Updated:') or line.startswith('Generated:'):
            return i
    return -1

def find_front_matter_end(lines):
    if not lines:
        return -1
    if lines[0].strip() != '---':
        return -1
    for i in range(1, min(len(lines), 200)):
        if lines[i].strip() == '---':
            return i
    return -1

def find_h1(lines):
    for i, line in enumerate(lines):
        if line.startswith('# '):
            return i
    return -1

SKIP_UPDATED = {
    os.path.join('docs', 'reference', 'gating_config.md').replace("\\", "/"),
    os.path.join('docs', 'reference', 'deprecations.md').replace("\\", "/"),
}


def extract_generated_metadata(lines: list[str]) -> tuple[int, str | None]:
    for i, line in enumerate(lines[:80]):
        if line.startswith('Generated:'):
            value = line.split(':', 1)[1].strip()
            date_part = value.split(' ', 1)[0] if value else ""
            return i, date_part or None
    return -1, None


def process(path: str) -> bool:
    rel = os.path.relpath(path, ROOT).replace("\\", "/")
    if rel in SKIP_UPDATED:
        return False
    try:
        with open(path, 'r', encoding='utf-8') as f:
            text = f.read()
    except Exception:
        return False
    lines = text.splitlines()
    idx = has_updated_or_generated(lines)
    updated_idx = idx if idx >= 0 and lines[idx].startswith('Updated:') else -1
    generated_idx, generated_date = extract_generated_metadata(lines)

    if generated_date:
        target_updated = generated_date
    else:
        target_updated = git_last_date(path)

    # If Updated exists and is different, refresh it in-place
    if updated_idx >= 0:
        cur = lines[updated_idx].split(':', 1)[1].strip()
        if cur != target_updated:
            lines[updated_idx] = f"Updated: {target_updated}"
            new_text = "\n".join(lines) + ("\n" if text.endswith('\n') else "")
            with open(path, 'w', encoding='utf-8') as f:
                f.write(new_text)
            return True
        return False

    # Otherwise, insert a new Updated: line. Prefer directly above Generated if present,
    # else after the first H1, else after front-matter block, else at top.
    if generated_idx >= 0:
        insert_at = generated_idx
    else:
        h1 = find_h1(lines)
        if h1 >= 0:
            insert_at = h1 + 1
        else:
            fm_end = find_front_matter_end(lines)
            insert_at = fm_end + 1 if fm_end >= 0 else 0

    new_lines = lines[:insert_at] + [f"Updated: {target_updated}"] + lines[insert_at:]
    new_text = "\n".join(new_lines) + ("\n" if text.endswith('\n') or not text else "")
    with open(path, 'w', encoding='utf-8') as f:
        f.write(new_text)
    return True

def main():
    changed = 0
    for root, _, files in os.walk(DOCS):
        for name in files:
            if not name.endswith('.md'):
                continue
            p = os.path.join(root, name)
            if process(p):
                print(f"[stamp] {os.path.relpath(p, ROOT)}")
                changed += 1
    print(f"Updated files: {changed}")

if __name__ == '__main__':
    sys.exit(main() or 0)
