---
title: Design System
---

# Design System

This documents the subtle visual language used across ARW UIs: calm, instrument-panel inspired, with copper/teal accents.

Updated: 2025-10-09
Type: How‑to

## Tokens

- Source of truth: [assets/design/tokens.css](https://github.com/t3hw00t/ARW/blob/main/assets/design/tokens.css) (synced via `just tokens-sync` to `docs/css/tokens.css`, `apps/arw-launcher/src-tauri/ui/tokens.css`, and `apps/arw-server/assets/ui/tokens.css`).
- UI kit primitives live at [assets/design/ui-kit.css](https://github.com/t3hw00t/ARW/blob/main/assets/design/ui-kit.css); run `just tokens-sync` to update `apps/arw-launcher/src-tauri/ui/ui-kit.css` and `apps/arw-server/assets/ui/ui-kit.css`.
- Prefer token variables over hex.
- Colors
- Primary (Copper): `var(--color-brand-copper)` (dark: `var(--color-brand-copper-dark)`, light: `#dca777`)
- Accent (Teal): `var(--color-accent-teal)` for fills, light `var(--color-accent-teal-light)` for gradients, and `var(--color-accent-teal-strong)` for text (≥4.9:1 on light surfaces, tuned per scheme); RGB helper `var(--color-accent-teal-strong-rgb)`
  - Ink/Muted/Line: `var(--color-ink)`, `var(--color-muted)`, `var(--color-line)`
  - Status: ok `var(--status-ok)` · warn `var(--status-warn)` · bad `var(--status-bad)` · info `var(--status-info)` (light surfaces stay ≥4.5:1); dark surfaces swap to `var(--status-*-dark)` and keep ≥5:1
  - Derived RGB helpers: `var(--status-ok-rgb)`, `var(--status-warn-rgb)`, `var(--status-bad-rgb)`, `var(--status-info-rgb)`, `var(--status-accent-rgb)`

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

- Expansive uses `var(--color-accent-teal-strong)` → `var(--color-accent-teal-light)`
- Complex uses `var(--color-brand-copper-dark)` → `var(--color-brand-copper)`
- Complicated blends copper to the strong accent for contrast on both themes

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

## Accessibility

- Minimum contrast: keep interactive text ≥4.5:1 against the background; status badges get dual encoding (colour + icon).
- Motion & flicker: avoid flashing elements; respect `prefers-reduced-motion` by disabling non-essential animations.
- Hit targets: primary buttons and critical toggles should provide ≥44×44 px hit areas; list items use at least 36 px height.
- Keyboard: every interactive element must be reachable via tab order; focus outlines should use the accent teal variant.
- Labels: pair icons with text or aria-labels; keep status announcements readable by screen readers.

## Examples

- Header badges showing SSE, Admin, Chat, Models, Memory, Governor.
- Box headers may include a minimal badge for local context (e.g., Memory limit).
