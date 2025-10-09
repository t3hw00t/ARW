---
title: Trial Visual Kit
---

# Trial Visual Kit

Updated: 2025-10-05
Type: Design brief

This brief keeps UI, docs, and training materials aligned while we polish the trial experience.

## Home screen

- **Tabs**: Overview, Workflows, Safeguards. Use friendly icons (home, compass, shield).
- **Status tray**: left-aligned pill with three indicators (System, Memory, Approvals). Use stoplight colours with plain-text labels (All good / Check soon / Action needed).
- **Preflight button**: primary button labeled “Run preflight” with a short caption underneath (“Takes ~30 seconds”).
- **What’s in focus card**: top-right card showing 3–5 key facts with timestamps and a “See sources” link.

## Approvals lane

- **Layout**: right-hand drawer; cards stack vertically.
- **Card content**: summary sentence, evidence preview (link or inline snippet), big Approve / Hold buttons, optional “Send back with notes.”
- **Tone**: use verbs (“Send follow-up email?”, “Sync supplier spreadsheet?”). Keep secondary details muted.
- **Empty state**: illustration + message “All clear. Helpers will pause here when they need you.”

## Trial Control Center

- **Tiles**: four equal cards (Systems, Memory, Approvals, Safety) with icon, headline metric, and single-line status. The Memory tile stacks two inline meters (“Coverage gaps” and “Recall risk”) so drift is visible without opening the tab details.
- **Pause/rollback**: two buttons beneath the tiles—Pause helpers (primary) and Roll back to last snapshot (secondary).
- **Autonomy placeholder**: faded card that says “Autonomy Lane (coming soon)” with link to charter.

## Printed/onboarding materials

- First-steps PDF mirrors the same tab names and icons.
- Quick-start card (fits on A5) with QR code linking to the runbook.
- Slide template for daily stand-ups shows the four tiles and a notes column.

## Accessibility & contrast

- Minimum 18px text on buttons, 14px on supporting labels.
- Colour palette meets WCAG AA for text/background; use dual encoding (colour + icon).
- Keyboard shortcut overlay triggered with “?” icon; list the top five shortcuts (e.g., preflight, open approvals, switch tabs).

## Deliverables

1. Figma page with the home screen, approvals drawer, and control center.
2. Exported PNGs for docs and training decks.
3. SVG icon set (home/compass/shield/status) checked into `assets/icons/trial/` (`home.svg`, `compass.svg`, `shield.svg`, `status.svg`).

## Implementation Notes

- The launcher Trial Control Center lives in `apps/arw-launcher/src-tauri/ui/trial.html` with companion CSS/JS. It renders the tabs, status tray, tiles, focus card, and preflight button described above.
- Preflight automation attempts to run `scripts/trials_preflight.ps1` on Windows or `scripts/trials_preflight.sh` elsewhere (falling back to `just trials-preflight`). When helper scripts are missing it falls back to `arw-cli smoke triad` / `arw-cli smoke context` so we always exercise the same action/state/context checks.
- Docs and ops runbooks link to the launcher window so rehearsals start from a single surface.

### Icon references

- Use the shared SVGs directly in docs or UI mockups: `assets/icons/trial/home.svg`, `assets/icons/trial/compass.svg`, `assets/icons/trial/shield.svg`, `assets/icons/trial/status.svg`.
- Each icon uses `currentColor`, so they inherit the surrounding text colour and remain high contrast across themes.
- When embedding in Markdown, include short alt text for accessibility, e.g. `![Home tab](../../assets/icons/trial/home.svg)`.

Keep the visuals approachable—friendly shapes, minimal chrome, and language that sounds like a teammate, not a terminal.
