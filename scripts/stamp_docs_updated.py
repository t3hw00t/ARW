#!/usr/bin/env python3
import subprocess, sys, os, datetime, re

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))
DOCS = os.path.join(ROOT, 'docs')

def git_last_date(path: str) -> str:
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
}


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
    updated_date = git_last_date(path)

    # If Updated exists and is different, refresh it in-place
    if idx >= 0 and lines[idx].startswith('Updated:'):
        cur = lines[idx].split(':', 1)[1].strip()
        if cur != updated_date:
            lines[idx] = f"Updated: {updated_date}"
            new_text = "\n".join(lines) + ("\n" if text.endswith('\n') else "")
            with open(path, 'w', encoding='utf-8') as f:
                f.write(new_text)
            return True
        return False

    # Otherwise, insert a new Updated: line after the first H1 if present,
    # else after front-matter block if present, else at top.
    insert_at = None
    h1 = find_h1(lines)
    if h1 >= 0:
        insert_at = h1 + 1
    else:
        fm_end = find_front_matter_end(lines)
        if fm_end >= 0:
            insert_at = fm_end + 1
        else:
            insert_at = 0

    # Ensure a blank line before and after for readability
    to_insert = [f"Updated: {updated_date}"]
    # insert a blank line if following line isn't blank
    new_lines = lines[:insert_at] + to_insert + lines[insert_at:]
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
