#!/usr/bin/env python3
"""
Scaffold a new endpoint: emit an OpenAPI snippet and a Rust utoipa wrapper.

Usage:
  python scripts/new_endpoint_template.py METHOD PATH [--tag TAG] [--operation-id ID]
         [--summary TEXT] [--description TEXT] [--deprecated] [--sunset ISO8601]
         [--apply]

When --apply is used, inserts the path into spec/openapi.yaml if absent and
runs ensure_openapi_descriptions to normalize tags/description.
"""
import argparse
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


def default_summary(method, path):
    return f"{method.upper()} {path}"


def default_opid(method, path):
    # e.g., GET /admin/world_diffs -> world_diffs_get_doc
    parts = [p for p in path.strip('/').split('/') if p and not p.startswith('{')]
    stem = '_'.join([p.replace('-', '_') for p in parts]) or 'root'
    return f"{stem}_{method.lower()}_doc"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('method')
    ap.add_argument('path')
    ap.add_argument('--tag')
    ap.add_argument('--operation-id')
    ap.add_argument('--summary')
    ap.add_argument('--description')
    ap.add_argument('--deprecated', action='store_true')
    ap.add_argument('--sunset', help='ISO8601, e.g., 2026-01-01T00:00:00Z')
    ap.add_argument('--apply', action='store_true')
    args = ap.parse_args()

    method = args.method.lower()
    path = args.path
    tag = args.tag
    opid = args.operation_id or default_opid(method, path)
    summary = args.summary or default_summary(method, path)
    desc = args.description or ''
    op = {
        'tags': [tag] if tag else [],
        'summary': summary,
        'operationId': opid,
        'responses': {
            '200': {'description': 'OK'}
        },
    }
    if desc:
        op['description'] = desc
    if args.deprecated:
        op['deprecated'] = True
        if args.sunset:
            op['x-sunset'] = args.sunset

    oas_snippet = {
        path: {
            method: op
        }
    }
    print('# OpenAPI snippet (YAML):\n')
    print(yaml.safe_dump(oas_snippet, sort_keys=False))

    rust_method = method
    rust_tag = tag or 'Public'
    print('\n# Rust utoipa wrapper (annotate a handler):\n')
    depo = ', deprecated = true' if args.deprecated else ''
    print(f"#[utoipa::path({rust_method}, path = \"{path}\", tag = \"{rust_tag}\"{depo}, responses((status=200, description=\"OK\")))]\nasync fn TODO_doc() -> impl IntoResponse {{ /* call real handler */ }}")

    if args.apply:
        doc = load_yaml(SPEC)
        paths = doc.setdefault('paths', {})
        if path in paths and method in (paths.get(path) or {}):
            print(f"\n[apply] Path {method.upper()} {path} already exists in spec. No changes.")
        else:
            entry = paths.setdefault(path, {})
            entry[method] = op
            save_yaml(SPEC, doc)
            print(f"\n[apply] Added {method.upper()} {path} to spec/openapi.yaml")
            # Run normalizer to ensure tags and description
            os.system(f"python3 {os.path.join(REPO, 'scripts', 'ensure_openapi_descriptions.py')} >/dev/null 2>&1 || true")

    return 0


if __name__ == '__main__':
    sys.exit(main())
