---
title: Trial Visual Kit
---

# Trial Visual Kit

Updated: 2025-09-26
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

- **Tiles**: four equal cards (Systems, Memory, Approvals, Safety) with icon, headline metric, and single-line status.
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
3. SVG icon set (home/compass/shield/status) checked into `assets/icons/trial/`.

Keep the visuals approachable—friendly shapes, minimal chrome, and language that sounds like a teammate, not a terminal.
