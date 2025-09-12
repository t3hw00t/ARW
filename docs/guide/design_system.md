---
title: Design System
---

# Design System

This documents the subtle visual language used across ARW UIs: calm, instrument-panel inspired, with copper/teal accents.

Updated: 2025-09-12

## Tokens

- Colors
  - Primary (copper): `#b87333` (light/dark: `#dca777`/`#915a29`)
  - Accent (teal): `#1bb3a3`
  - Ink: `#111827` · Muted: `#6b7280` · Line: `#e5e7eb`
  - Status: ok `#22c55e` · warn `#f59e0b` · bad `#ef4444`

- Radii
  - Panels: `10px` · Buttons: `8px` · Badges: `999px`

- Components
  - Badges: pill with small dot indicator and muted backgrounds; ok/warn/bad variants.
  - Panels: soft inset stroke and rounded corners; avoid strong shadows.
  - Buttons: neutral surfaces with subtle hover; avoid heavy borders.

## Usage

- Use neutral surfaces by default; reserve color for status and accents.
- Prefer compact dot badges for live state (Admin/SSE/Chat/Models/Memory/Governor).
- Avoid blinking or high-contrast flicker; transitions should be calm and brief.
- Keep layout steady; cluster related actions in “rows”, keep rhythm consistent.

## Examples

- Header badges showing SSE, Admin, Chat, Models, Memory, Governor.
- Box headers may include a minimal badge for local context (e.g., Memory limit).
