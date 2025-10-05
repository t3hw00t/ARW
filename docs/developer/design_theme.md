---
title: Design Theme
---

# Design Theme

Updated: 2025-09-29
Type: Reference

Microsummary: A shared visual and language system across docs, apps, and debug UIs — consistent naming, tone, layout, tokens, and colors. Stable.

See also: [Style & Harmony](style.md), [UX Invariants](../architecture/ux_invariants.md), [Workflow Views](../guide/workflow_views.md)

## Naming

- Product: Agent Hub (ARW). Use “Launcher” for the Tauri app, “Service” for `arw-server`, and “CLI” for `arw-cli`.
- Views: Project Hub, Chat, Training Park, Events, Logs, Debug.
- Actions: use verbs users know (Open, Start, Stop, Refresh, Copy, Replay). Avoid jargon.
- Events: dot‑case with segments, past/present tense by kind: `models.download.progress`, `models.changed`, `feedback.suggested`, `actions.applied`.
- Status: ok, warn, bad, info, accent. Don’t invent new severity words.
- API/Specs: follow existing conventions (ProblemDetails for errors, snake_case `operationId` ending with `_doc`).

## Language & Tone

- US English. Examples: canceled, color, disk.
- Calm, helpful, action‑oriented. Avoid blame; offer a next step.
- Microcopy: short, scannable, consistent nouns/verbs across UI, events, APIs.
- Errors: one‑line summary + cause + suggested action.

## Tokens & Theming

- Canonical tokens (CSS): `docs/css/tokens.css` defines brand, surfaces, status tones, spacing, radii, shadows, and MkDocs bridges.
- Canonical tokens (JSON): `docs/design/tokens.json` mirrors the palette and scales for non‑CSS consumers.
- Docs usage: `mkdocs.yml` includes `css/tokens.css` then `css/overrides.css` (overrides can fine‑tune visuals).
- Apps usage: prefer CSS variables with the same names. If bundling constraints exist, copy the token block verbatim to app CSS.

Key CSS variables
- Brand: `--color-brand-copper`, `--color-brand-copper-dark`, `--color-accent-teal`, `--color-accent-teal-light`, `--color-accent-teal-strong` (+ `--color-accent-teal-strong-rgb`).
- Surfaces/Neutrals: `--surface`, `--surface-muted`, `--color-ink`, `--color-muted`, `--color-line`.
- Status: `--status-ok`, `--status-warn`, `--status-bad`, `--status-info`, `--status-accent` (+ matching `--status-*-rgb` helpers and dark overrides `--status-*-dark`).
- Rhythm: `--sp2`, `--sp3`, `--sp4`, `--sp5`; Radii: `--radius-2/3/4`; Shadows: `--shadow-1/2/3`.
- Dark mode: honors `prefers-color-scheme`; tokens swap neutrals/surfaces only — keep accents stable.

## Colors

- Brand primary: Copper `#b87333` (dark `#915a29`).
- Accent: Teal `#1bb3a3` (light `#63d3c9`, strong text tone `#0f766e` light / `#2dd4bf` dark).
- Neutrals: Ink `#111827`, Muted `#4b5563` (dark `#94a3b8`), Line `#e5e7eb`.
- Surfaces: Page `#ffffff`, Muted `#fafaf9` (dark: `#0f1115`, `#0b0d11`).
- Status: ok `#15803d`, warn `#b45309`, bad `#a91d1d`, info `#2563eb`; dark overrides `#34d399` / `#fbbf24` / `#fca5a5` / `#60a5fa`; RGB tuples exposed via `--status-*-rgb` for overlays.

Usage
- Primary CTAs use Copper; secondary actions default; low‑emphasis use Ghost.
- Progress/positive use ok; destructive use bad; warnings use warn; neutral informational use info.

## Typography

- Font: system UI stack for app and docs; monospace for code/samples.
- Size: responsive base 14–16px; headings use Material defaults in docs and consistent sizes in apps.
- Emphasis: bold and callouts sparingly; prefer structure and whitespace.

## Layout & Rhythm

- Spacing: use `--sp2/3/4/5` consistently; avoid one‑off paddings.
- Radii: 6px/8px/12px; avoid mixing more than two radii in a view.
- Elevation: use `--shadow-1` for interactive elements, `--shadow-2` for content containers, `--shadow-3` for overlays.
- Invariants: universal right‑sidecar with lanes for Timeline, Context, Policy, Metrics, Models, Activity.
- Density: optional “Focus/Compact” mode may reduce gaps and radii; persist per view.

## Components

- Buttons: `.primary` (Copper grad), `.ghost` (transparent with border), default (textured light surface). Respect hover/active motion by a fraction of a pixel.
- Inputs: rounded 8px, clear focus ring, subtle inner highlight; `:focus-visible` outline uses Copper tone.
- Badges/Chips: rounded (9999px), thin borders matching `--color-line`.
- Bars/Progress: gradient accent; keep heights 6–10px with rounded corners.
- Command Palette: modal overlay blur with saturated background, list items hover highlight.

## Motion

- Reduce motion: honor `prefers-reduced-motion` (disable transitions/animations).
- Default micro‑motion: 60–120ms for hover/focus, 120–200ms for overlays.
- Avoid springy bounces and large parallax; stay calm and subtle.

## Accessibility

- Contrast: meet WCAG AA for text and controls; Copper needs the lighter variant on very dark surfaces. Status colors (#15803d / #b45309 / #a91d1d / #2563eb) clear ≥4.5:1 on light surfaces, while dark-mode overrides (#34d399 / #fbbf24 / #fca5a5 / #60a5fa) stay ≥5:1 against dark neutrals.
- Focus: strong visible outline on interactive elements; use `:focus-visible`.
- Keyboard: palette is Cmd/Ctrl‑K; Escape closes overlays; Tab order is logical.
- Color alone: never the only signal; combine with icons/text.

## States & Semantics

- Status tones map: ok=success, warn=degraded/attention, bad=error, info=neutral/explanatory, accent=highlight.
- Error copy: say what happened, why, and what to do next.
- Empty states: show what belongs here + first step link; avoid blank panels.

## Implementation Notes

- Single‑source tokens live under `assets/design/` (CSS + JSON; plus W3C format). Run `just tokens-sync` to copy them to docs and the launcher UI.
- To regenerate CSS/JSON from the W3C tokens, run `just tokens-build` (or `just tokens-rebuild` to build+sync+check).
- Docs site loads `docs/css/tokens.css` via `mkdocs.yml → extra_css`.
- Launcher app links `tokens.css`, then `ui-kit.css`, then `common.css` on every page.
- Debug UI (service) has a minimal inline token set; can be expanded as more elements adopt tokens.
- Optional: class‑based theming via SD outputs — include `assets/design/generated/tokens.theme.css` and toggle `.theme-light`/`.theme-dark` on a root container (supplementary to `prefers-color-scheme`).
- Density: compact spacing can be toggled per page; stored under `localStorage['arw:density:<page>']` and applies `body.compact` (resets `--sp2/3/4/5`).
 - Focus Mode: per page under `localStorage['arw:focus:<page>']`; toggles `.layout.full`.

### Theme override (optional)

Prefer OS dark/light (via `prefers-color-scheme`) for coherence. If a manual override is required in a specific surface, keep it scoped and persistent.

HTML
```html
<link rel="stylesheet" href="/assets/design/generated/tokens.theme.css" />
<body class="theme-light"> ... </body>
```

JS
```js
// Toggle and persist theme override (Auto uses OS)
const THEME_KEY = 'arw:theme'; // 'auto' | 'light' | 'dark'
function applyTheme(val){
  const el = document.body; el.classList.remove('theme-light','theme-dark');
  if (val === 'light') el.classList.add('theme-light');
  else if (val === 'dark') el.classList.add('theme-dark');
  // auto: no class; OS preference + default CSS applies
}
applyTheme(localStorage.getItem(THEME_KEY) || 'auto');
```

Notes
- Do not mix multiple theming strategies across surfaces. Keep OS‑first; use class override only where absolutely needed.
- Avoid fighting `@media (prefers-color-scheme: dark)` rules. Class override should only set neutrals (surface/ink/line).

Open standards
- W3C Design Tokens (Format Module Level 1) — exported at `assets/design/tokens.w3c.json` for downstream pipelines (e.g., Style Dictionary) to generate platform-specific outputs.

## Quick Reference

- CSS tokens: docs/css/tokens.css
- JSON tokens: docs/design/tokens.json
- Style & Harmony: [style.md](style.md)
- UX Invariants: [architecture/ux_invariants.md](../architecture/ux_invariants.md)
- Workflow Views: [guide/workflow_views.md](../guide/workflow_views.md)
