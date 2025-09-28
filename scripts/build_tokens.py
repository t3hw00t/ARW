#!/usr/bin/env python3
import json, os, re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / 'assets/design/tokens.w3c.json'
OUT_CSS = ROOT / 'assets/design/tokens.css'
OUT_JSON = ROOT / 'assets/design/tokens.json'

def hex_to_rgb_tuple(hx: str):
    hx = hx.strip().lstrip('#')
    if len(hx) == 3:
        hx = ''.join([c*2 for c in hx])
    r = int(hx[0:2], 16)
    g = int(hx[2:4], 16)
    b = int(hx[4:6], 16)
    return (r, g, b)

def main():
    data = json.loads(SRC.read_text())

    # Flatten W3C tokens into our CSS var names
    css_vars = {}
    json_out = {
        'brand': {}, 'neutrals': {}, 'surfaces': {}, 'status': {},
        'spacing': {}, 'radii': {}, 'shadows': {}
    }

    # Colors → CSS vars
    c = data.get('color', {})
    brand = c.get('brand', {})
    neutrals = c.get('neutrals', {})
    surfaces = c.get('surfaces', {})
    status = c.get('status', {})
    # Brand
    if 'copper' in brand: css_vars['--color-brand-copper'] = brand['copper']['$value']
    if 'copper_dark' in brand: css_vars['--color-brand-copper-dark'] = brand['copper_dark']['$value']
    if 'teal' in brand: css_vars['--color-accent-teal'] = brand['teal']['$value']
    if 'teal_light' in brand: css_vars['--color-accent-teal-light'] = brand['teal_light']['$value']
    # Neutrals & surfaces
    if 'ink' in neutrals: css_vars['--color-ink'] = neutrals['ink']['$value']
    if 'muted' in neutrals: css_vars['--color-muted'] = neutrals['muted']['$value']
    if 'line' in neutrals: css_vars['--color-line'] = neutrals['line']['$value']
    if 'surface' in surfaces:
        css_vars['--surface'] = surfaces['surface']['$value']
    if 'surface_muted' in surfaces:
        css_vars['--surface-muted'] = surfaces['surface_muted']['$value']
    # Status
    for k, v in status.items(): css_vars[f'--status-{k}'] = v['$value']

    # Spacing
    for k, v in (data.get('spacing', {})).items(): css_vars[f'--sp{re.sub(r"^sp?", "", k)}' if k.startswith('sp') else f'--sp{k}'] = v['$value']
    # Radii
    for k, v in (data.get('radii', {})).items(): css_vars[f'--radius-{re.sub(r"^r", "", k)}'] = v['$value']
    # Shadows
    for k, v in (data.get('shadows', {})).items(): css_vars[f'--{k.replace("_", "-")}'] = v['$value']

    # Derived
    if '--color-brand-copper' in css_vars:
        r, g, b = hex_to_rgb_tuple(css_vars['--color-brand-copper'])
        css_vars['--brand-copper-rgb'] = f'{r},{g},{b}'

    # Noise texture (constant)
    css_vars['--noise'] = "url(\"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='140' height='140' viewBox='0 0 140 140'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='2' stitchTiles='stitch'/%3E%3CfeColorMatrix type='saturate' values='0'/%3E%3CfeComponentTransfer%3E%3CfeFuncA type='table' tableValues='0 0.035'/%3E%3C/feComponentTransfer%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E\")"

    # Dark mode overrides (derived from neutrals)
    dark_overrides = {
        '--surface': '#0f1115',
        '--surface-muted': '#0b0d11',
        '--color-ink': '#e5e7eb',
        '--color-line': '#1f232a',
    }

    # Write CSS
    lines = [":root{\n"]
    for k in sorted(css_vars.keys()):
        lines.append(f"  {k}: {css_vars[k]};\n")
    lines.append("}\n\n@media (prefers-color-scheme: dark){\n  :root{\n")
    for k, v in dark_overrides.items(): lines.append(f"    {k}: {v};\n")
    lines.append("  }\n}\n")
    lines.append("\n@media (prefers-contrast: more){\n")
    lines.append("  :root{\n")
    lines.append("    --color-line: currentColor;\n")
    lines.append("    --color-muted: #1f2937;\n")
    lines.append("    --shadow-1: none;\n")
    lines.append("    --shadow-2: none;\n")
    lines.append("    --shadow-3: none;\n")
    lines.append("    --noise: none;\n")
    lines.append("  }\n}\n")
    lines.append("\n@media (forced-colors: active){\n")
    lines.append("  :root{\n")
    lines.append("    --noise: none;\n")
    lines.append("  }\n}\n")
    OUT_CSS.write_text(''.join(lines))

    # Write JSON mirror (simple map matching current docs/apps expectations)
    json_out['brand'] = {
        'copper': css_vars.get('--color-brand-copper', ''),
        'copper_dark': css_vars.get('--color-brand-copper-dark', ''),
        'teal': css_vars.get('--color-accent-teal', ''),
        'teal_light': css_vars.get('--color-accent-teal-light', ''),
    }
    json_out['neutrals'] = {
        'ink': css_vars.get('--color-ink', ''),
        'muted': css_vars.get('--color-muted', ''),
        'line': css_vars.get('--color-line', ''),
    }
    json_out['surfaces'] = {
        'surface': css_vars.get('--surface', ''),
        'surface_muted': css_vars.get('--surface-muted', ''),
        'dark_surface': dark_overrides['--surface'],
        'dark_surface_muted': dark_overrides['--surface-muted'],
    }
    json_out['status'] = {
        'ok': css_vars.get('--status-ok', ''),
        'warn': css_vars.get('--status-warn', ''),
        'bad': css_vars.get('--status-bad', ''),
        'info': css_vars.get('--status-info', ''),
        'accent': css_vars.get('--color-accent-teal', ''),
    }
    json_out['spacing'] = { 'sp2': 8, 'sp3': 12, 'sp4': 16, 'sp5': 24 }
    json_out['radii'] = { 'r2': 6, 'r3': 8, 'r4': 12 }
    json_out['shadows'] = {
        'shadow_1': css_vars.get('--shadow-1', ''),
        'shadow_2': css_vars.get('--shadow-2', ''),
        'shadow_3': css_vars.get('--shadow-3', ''),
    }
    OUT_JSON.write_text(json.dumps(json_out, indent=2) + "\n")

if __name__ == '__main__':
    main()
