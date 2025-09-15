---
title: Design System
---

# Design System

This documents the subtle visual language used across ARW UIs: calm, instrument-panel inspired, with copper/teal accents.

Updated: 2025-09-12
Type: How‑to

## Tokens

- Source of truth: `assets/design/tokens.css` (synced to docs at `docs/css/tokens.css`).
- Prefer token variables over hex.
- Colors
  - Primary (Copper): `var(--color-brand-copper)` (dark: `var(--color-brand-copper-dark)`, light: `#dca777`)
  - Accent (Teal): `var(--color-accent-teal)`, light `var(--color-accent-teal-light)`
  - Ink/Muted/Line: `var(--color-ink)`, `var(--color-muted)`, `var(--color-line)`
  - Status: ok `var(--status-ok)` · warn `var(--status-warn)` · bad `var(--status-bad)` · info `var(--status-info)`

- Radii
  - Panels: `10px` · Buttons: `8px` · Badges: `999px`

- Components
  - Badges: pill with small dot indicator and muted backgrounds; ok/warn/bad variants.
  - Panels: soft inset stroke and rounded corners; avoid strong shadows.
  - Buttons: neutral surfaces with subtle hover; avoid heavy borders.

## Rhythm & Spacing

- Spacing scale (CSS vars): `--sp2:8px`, `--sp3:12px`, `--sp4:16px`, `--sp5:24px`.
- Density toggle: views may provide a “Density” control; compact sets smaller gaps and radii for dense screens.
- Grouping: related actions sit on one row; repeated page sections are collapsible to reduce scrolling.

## Usage

- Use neutral surfaces by default; reserve color for status and accents.
- Prefer compact dot badges for live state (Admin/SSE/Chat/Models/Memory/Governor).
- Avoid blinking or high-contrast flicker; transitions should be calm and brief.
- Keep layout steady; cluster related actions in “rows”, keep rhythm consistent.

## Buttons

- Variants: `.primary` for key actions, base (neutral) for normal actions, `.ghost` for low‑emphasis or reversible actions.
- Don’t overuse `.primary`; each view should have a clear CTA.

## Topic markers

Use the trio badge to show “expansive / complex / complicated” at a glance. Always rendered as three gradient segments; tune each segment’s strength (0..1) with CSS variables and mirror values in `data-*` for readable labels.

```
## Title of Section
{ .topic-trio style="--exp:.8; --complex:.5; --complicated:.3" data-exp=".8" data-complex=".5" data-complicated=".3" }
```

Defaults (if omitted): `--exp:.66; --complex:.66; --complicated:.66`.

Notes:
- Works on `h1`–`h4` and appends a compact pill.
- Segments and colors align to the theme (teal for expansive, copper for complex, blend for complicated).
- Keep usage sparing; typically once per page or per major section.

## Iconography

- Simple states use one compact icon. Complex states use a small icon set, still subtle.
- Status tones: ok (green), warn (amber), bad (red), accent (teal), info (muted).
- Examples (in UI code): `check` for complete, `download` for in‑progress, `refresh` for resumed/resync, `timer` for degraded/slow.

## Panels & Collapsibles

- Panel headers can toggle collapse. Persist the collapsed state (e.g., `localStorage`) per heading text.
- Provide “Expand/Collapse all” on dashboards with many panels.

## Dark Mode

- Honor `prefers-color-scheme: dark` with the same hierarchy and accents; keep shadows subtle.

## Examples

- Header badges showing SSE, Admin, Chat, Models, Memory, Governor.
- Box headers may include a minimal badge for local context (e.g., Memory limit).
