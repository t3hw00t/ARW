---
title: Design Overview
---

# Design Overview

Updated: 2025-10-05
Type: Landing page

This page collects the design-facing surfaces so teams can find shared assets, briefs, and implementation guidance quickly.

## Core Resources

- [Design System Guide](../guide/design_system.md) — tokens, typography, and UI primitives for ARW surfaces.
- [Design Theme & Tokens](../developer/design_theme.md) — the source of truth behind the CSS variables and theming presets.
- Design tokens JSON (`tokens.json`) — feed the latest colors, spacing, and radii into design tools. The file stays in sync with the generator under `just tokens-*`.
- [Trial Visual Kit](trial_visual_kit.md) — mock and asset checklist for the Trial Control Center experience.
- [Autonomy Recovery Follow-ups](autonomy_recovery_followups.md) — implementation record that documents the recovery lane APIs and operator flows.

## Assets & Tooling

- SVG icon sets live under `assets/icons/`; the trial-specific icons referenced in the visual kit are in `assets/icons/trial/` and use `currentColor` for easy theming.
- Run `just tokens-build` after updating `assets/design/tokens.json` to regenerate CSS variables (`docs/css/tokens.css`) and keep docs + launcher styles aligned.
- `just tokens-check` verifies token JSON stays sorted and matches the generated CSS snapshot; run it before committing token updates.

## Collaboration Notes

- Keep design briefs under `docs/design/` with metadata headers so the docs site surfaces the latest update date automatically.
- When adding new design deliverables, link their assets or Figma sources here and cross-link from the relevant guides (for example, Launcher or Docs style guides).
- Accessibility requirements belong in each brief; reuse the [Design System Guide accessibility checklist](../guide/design_system.md#accessibility) so expectations stay consistent across surfaces.

## Implementation Hooks

- Launcher UI code lives under `apps/arw-launcher/src-tauri/ui/`; align CSS class names with the tokens exported by `tokens.css` to avoid bespoke styles.
- Docs and runbooks embed the same assets; prefer relative links (`../../assets/...`) so the build remains portable.
- Feature flags or config toggles referenced in briefs should map to entries in `docs/reference/feature_matrix.md` and `docs/reference/feature_catalog.md` so the broader roadmap stays coherent.

Use this page as the jumping-off point when onboarding designers or reviewing design-impacting changes. Add to it whenever a new brief or shared asset lands.
