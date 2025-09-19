#!/usr/bin/env python3
import os, sys, re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DOCS = ROOT / 'docs'

def detect_type(path: Path) -> str:
    p = str(path).replace('\\', '/').lower()
    name = path.name.lower()
    # Specifics first
    if p.endswith('/guide/quickstart.md'): return 'Tutorial'
    if p.endswith('/guide/concepts.md'): return 'Explanation'
    # Folders
    if '/architecture/' in p: return 'Explanation'
    if '/guide/' in p: return 'How‑to'
    if '/reference/' in p: return 'Reference'
    if '/api/' in p: return 'Reference'
    if '/developer/' in p: return 'Reference'
    if '/ai/' in p: return 'Reference'
    if '/ops/' in p: return 'How‑to'
    if '/ethics/' in p: return 'Explanation'
    # Roots / filenames
    if name == 'index.md': return 'Explanation'
    root_map = {
        'features.md': 'Explanation',
        'glossary.md': 'Reference',
        'configuration.md': 'Reference',
        'api_and_schema.md': 'Reference',
        'interface_roadmap.md': 'Reference',
        'roadmap.md': 'Reference',
        'release_notes.md': 'Reference',
        'backlog.md': 'Reference',
        'gating_keys.md': 'Reference',
        'structure.md': 'Explanation',
        'hierarchy.md': 'Explanation',
        'world_model.md': 'Explanation',
        'hardware_and_models.md': 'Explanation',
        'memory_and_training.md': 'Explanation',
        'policy.md': 'Explanation',
        'project_instructions.md': 'How‑to',
        'clustering.md': 'Explanation',
        'arrow_ingestion.md': 'How‑to',
        'training_research.md': 'Explanation',
    }
    return root_map.get(name, 'Explanation')

def has_type(lines):
    for i, line in enumerate(lines[:60]):
        if line.strip().lower().startswith('type:'):
            return i
    return -1

def find_updated(lines):
    for i, line in enumerate(lines[:60]):
        if line.startswith('Updated:'):
            return i
    return -1

def find_front_matter_end(lines):
    if not lines or lines[0].strip() != '---':
        return -1
    for i in range(1, min(len(lines), 200)):
        if lines[i].strip() == '---':
            return i
    return -1

def find_h1(lines):
    for i, line in enumerate(lines[:120]):
        if line.startswith('# '):
            return i
    return -1

def insert_type(path: Path) -> bool:
    try:
        text = path.read_text(encoding='utf-8')
    except Exception:
        return False
    lines = text.splitlines()
    if has_type(lines) >= 0:
        return False
    tval = detect_type(path)
    if not tval:
        return False

    # Insert after Updated: if present, else after H1, else after front matter, else at top
    ua = find_updated(lines)
    if ua >= 0:
        insert_at = ua + 1
    else:
        h1 = find_h1(lines)
        if h1 >= 0:
            insert_at = h1 + 1
        else:
            fm_end = find_front_matter_end(lines)
            insert_at = (fm_end + 1) if fm_end >= 0 else 0

    new_lines = lines[:insert_at] + [f'Type: {tval}'] + lines[insert_at:]
    path.write_text('\n'.join(new_lines) + ('\n' if text.endswith('\n') or not text else ''), encoding='utf-8')
    return True

def main():
    changed = 0
    for root, _, files in os.walk(DOCS):
        for name in files:
            if not name.endswith('.md'):
                continue
            p = Path(root) / name
            if insert_type(p):
                rel = p.relative_to(ROOT)
                print(f'[type] {rel}')
                changed += 1
    print(f'Updated files: {changed}')

if __name__ == '__main__':
    main()
