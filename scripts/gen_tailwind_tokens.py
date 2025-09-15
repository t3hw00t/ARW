#!/usr/bin/env python3
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TOKENS_JSON = ROOT / 'assets/design/tokens.json'
OUT = ROOT / 'assets/design/tailwind.tokens.json'

def main():
    t = json.loads(TOKENS_JSON.read_text())
    colors = {}
    # Brand and status
    for k, v in t.get('brand', {}).items():
        colors[f'brand-{k}'] = v
    for k, v in t.get('status', {}).items():
        colors[f'status-{k}'] = v
    # Neutrals
    for k, v in t.get('neutrals', {}).items():
        colors[k] = v
    # Surfaces
    for k, v in t.get('surfaces', {}).items():
        colors[k] = v
    out = { 'theme': { 'colors': colors } }
    OUT.write_text(json.dumps(out, indent=2))
    print(f"Wrote {OUT}")

if __name__ == '__main__':
    main()

