#!/usr/bin/env python3
"""
Ensure OpenAPI has brief operation descriptions and that all used tags are defined.

Edits spec/openapi.yaml in-place when changes are needed.
Requires: PyYAML
"""
import os
import sys
import yaml

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SPEC = os.path.join(REPO, 'spec', 'openapi.yaml')


def load_yaml(path):
    with open(path, 'r', encoding='utf-8') as f:
        return yaml.safe_load(f)


def save_yaml(path, obj):
    with open(path, 'w', encoding='utf-8') as f:
        yaml.safe_dump(obj, f, sort_keys=False)


def ensure_tags_defined(doc, used_tags):
    info = doc.setdefault('info', {})
    tags = info.get('tags') or doc.get('tags')  # allow top-level tags
    if tags is None:
        # Prefer top-level tags (OpenAPI supports top-level `tags`)
        tags = []
        doc['tags'] = tags
    name_to_idx = {t.get('name'): i for i, t in enumerate(tags) if isinstance(t, dict) and 'name' in t}
    changed = False
    for tag in sorted(used_tags):
        if tag and tag not in name_to_idx:
            tags.append({'name': tag, 'description': f'{tag} endpoints'})
            changed = True
    return changed


def ensure_operation_tags(op, path):
    if op.get('tags'):
        return None
    # Heuristic mapping
    m = [
        ('/admin/models', 'Admin/Models'),
        ('/admin/memory/quarantine', 'Admin/Review'),
        ('/admin/memory', 'Admin/Memory'),
        ('/admin/introspect', 'Admin/Introspect'),
        ('/admin/tools', 'Admin/Tools'),
        ('/admin/governor', 'Admin/Governor'),
        ('/admin/hierarchy', 'Admin/Hierarchy'),
        ('/admin/experiments', 'Admin/Experiments'),
        ('/admin/goldens', 'Admin/Goldens'),
        ('/admin/distill', 'Admin/Distill'),
        ('/admin/safety', 'Admin/Safety'),
        ('/admin/self_model', 'Admin/SelfModel'),
        ('/admin/world_diffs', 'Admin/Review'),
        ('/admin/tasks', 'Admin/Tasks'),
        ('/admin/probe', 'Admin/Introspect'),
        ('/admin', 'Admin/Core'),
        ('/state/projects', 'State/Projects'),
        ('/state', 'State/Core'),
        ('/projects', 'Projects'),
        ('/spec', 'Public/Specs'),
        ('/catalog', 'Public/Specs'),
    ]
    tag = 'Public'
    for pfx, t in m:
        if path.startswith(pfx):
            tag = t
            break
    op['tags'] = [tag]
    return tag


def short_desc(tag, method, path, summary=None):
    pref = ''
    if summary:
        return summary if len(summary) <= 140 else summary[:137] + '...'
    if tag:
        pref = f'{tag.split("/")[-1]}: '
    verb = method.upper()
    return f'{pref}{verb} {path}.'


def main():
    doc = load_yaml(SPEC)
    changed = False
    used_tags = set()
    paths = doc.get('paths') or {}
    for path, ops in paths.items():
        if not isinstance(ops, dict):
            continue
        for method, op in ops.items():
            m = str(method).lower()
            if m not in ('get','post','put','delete','patch','head','options','trace'):
                continue
            if not isinstance(op, dict):
                continue
            # Ensure tags
            added_tag = ensure_operation_tags(op, path)
            if added_tag is not None:
                changed = True
            tags = op.get('tags') or []
            if tags:
                used_tags.update(x for x in tags if isinstance(x, str))
            if not op.get('description'):
                op['description'] = short_desc((tags[0] if tags else ''), m, path, op.get('summary'))
                changed = True
    if ensure_tags_defined(doc, used_tags):
        changed = True
    if changed:
        save_yaml(SPEC, doc)
        print('updated spec/openapi.yaml')
        return 0
    print('no changes')
    return 0


if __name__ == '__main__':
    sys.exit(main())
