#!/usr/bin/env python3
"""
Merge curated spec fields into code-generated OpenAPI and validate path parity.

Usage: openapi_overlay.py <codegen_yaml> <curated_yaml> <out_yaml>

Rules:
- Validate that the set of paths + methods are identical between codegen and curated.
- Copy curated top-level `info` and `tags` into the merged output.
- For each path+method, copy curated fields that are human-facing:
  summary, description, deprecated, tags, x-sunset, parameters, responses.
- Replace `components` with curated `components` to keep schema canonical.
"""
import sys
import yaml

METHODS = {"get","post","put","delete","patch","options","head","trace"}
MERGE_KEYS = [
    "summary","description","deprecated","tags","x-sunset","parameters","responses"
]

def load_yaml(p):
    with open(p, 'r', encoding='utf-8') as f:
        return yaml.safe_load(f)

def save_yaml(p, obj):
    with open(p, 'w', encoding='utf-8') as f:
        yaml.safe_dump(obj, f, sort_keys=False)

def path_ops(spec):
    out = set()
    for path, node in (spec.get('paths') or {}).items():
        if not isinstance(node, dict):
            continue
        for m in node:
            ml = str(m).lower()
            if ml in METHODS:
                out.add((path, ml))
    return out

def ensure_path_parity(code, curated):
    a = path_ops(code)
    b = path_ops(curated)
    only_a = sorted(list(a - b))
    only_b = sorted(list(b - a))
    if only_a or only_b:
        lines = []
        if only_a:
            lines.append("Extra in codegen (missing in spec):" )
            lines += [f"  {p} {m}" for (p,m) in only_a]
        if only_b:
            lines.append("Missing in codegen (present in spec):")
            lines += [f"  {p} {m}" for (p,m) in only_b]
        raise SystemExit("Path parity mismatch\n" + "\n".join(lines))

def merge_overlay(code, curated):
    out = dict(code)
    out['info'] = curated.get('info')
    out['tags'] = curated.get('tags')
    # Paths: copy selected keys from curated into codegen
    paths = {}
    for path, node in (code.get('paths') or {}).items():
        node2 = {}
        for m, op in node.items():
            ml = str(m).lower()
            if ml not in METHODS:
                continue
            merged = dict(op or {})
            cur = ((curated.get('paths') or {}).get(path) or {}).get(m, {})
            for k in MERGE_KEYS:
                if k in (cur or {}):
                    merged[k] = cur[k]
            node2[m] = merged
        if node2:
            paths[path] = node2
    out['paths'] = paths
    # components from curated
    if 'components' in curated:
        out['components'] = curated['components']
    return out

def main():
    if len(sys.argv) != 4:
        print(__doc__)
        sys.exit(2)
    code_p, cur_p, out_p = sys.argv[1:]
    code = load_yaml(code_p)
    curated = load_yaml(cur_p)
    ensure_path_parity(code, curated)
    merged = merge_overlay(code, curated)
    save_yaml(out_p, merged)

if __name__ == '__main__':
    main()

