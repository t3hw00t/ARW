---
title: UI Kit (Launcher)
---

# UI Kit (Launcher)

Updated: 2025-09-16
Type: How‑to

Microsummary: Reuse small, consistent primitives across launcher pages — cards, buttons, inputs, badges/pills, meters — all styled via shared design tokens.

See also: developer/design_theme.md

## Include Order

- Each page links styles in this order:
  1. `tokens.css` (design tokens)
  2. `ui-kit.css` (primitives)
  3. `common.css` (page layout helpers, sidecar, palette, compare)

## Primitives

- Card: `.card` — bordered container, soft gradient, 12px radius, `--shadow-2`.
- Buttons:
  - Base: `button` — neutral surface, subtle hover/active
  - Primary: `button.primary` — Copper gradient; use sparingly for CTAs
  - Ghost: `button.ghost` — transparent, low‑emphasis
- Inputs: `input, select, textarea` — rounded 8px, clear focus ring using Copper
- Badges: `.badge` — pill with optional `.dot` status indicator
- Pills: `.pill` — inline filter/status
- Meter: `.bar > i` — tokenized accent gradient
- Status helpers: `.ok`, `.warn`, `.bad`, `.dim`

## Tokens

- Source of truth: `assets/design/tokens.css` (synced to `apps/arw-launcher/src-tauri/ui/tokens.css`)
- Use tokens instead of hex colors. Key variables:
  - Brand: `--color-brand-copper`, `--color-brand-copper-dark`, `--color-accent-teal`, `--color-accent-teal-light`
  - Neutrals: `--color-ink`, `--color-muted`, `--color-line`
  - Surfaces: `--surface`, `--surface-muted`
  - Status: `--status-ok`, `--status-warn`, `--status-bad`, `--status-info`
  - Rhythm: `--sp2/3/4/5`; Radii: `--radius-2/3/4`; Shadows: `--shadow-1/2/3`

## Accessibility

- Focus: `:focus-visible` outlines use `--brand-copper-rgb` for a clear ring.
- Motion: honor `prefers-reduced-motion: reduce`.
- Contrast: favor readable combinations; avoid low‑contrast on muted surfaces.

## Example

```html
<link rel="stylesheet" href="tokens.css" />
<link rel="stylesheet" href="ui-kit.css" />
<div class="card">
  <h3>Models <span class="badge"><span class="dot ok"></span>live</span></h3>
  <div class="row">
    <input placeholder="Filter" />
    <button class="ghost">Refresh</button>
    <button class="primary">Download</button>
  </div>
  <div class="bar"><i style="width:42%"></i></div>
  <div class="dim">P95: 2.1s</div>
  <div class="toast-wrap"><div class="toast">Models updated</div></div>
  </div>
```

