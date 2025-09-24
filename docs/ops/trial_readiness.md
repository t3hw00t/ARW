---
title: Trial Readiness Plan
---

# Trial Readiness Plan

Updated: 2025-09-26
Type: Playbook (quick guide)

Use this short guide to decide when Agent Hub feels good enough to invite real teammates into a trial. It sticks to plain language so product owners, operators, and partners can follow along without digging into the code.

## Four easy checkpoints

| Gate | Focus | What users should notice | Quick prep for the crew |
| ---- | ----- | ------------------------ | ------------------------ |
| **G0 · Core feels steady** | Launch & status lights | Home opens quickly, status tray says “All good,” the self-check button passes. | Single activity history, refreshed sign-in tokens, preflight button wired. |
| **G1 · Memory makes sense** | Briefs & context | “What’s in focus” card shows key facts with timestamps and sources. | Summaries stay fresh, gentle “needs background” nudges appear, metrics hit the dashboard. |
| **G2 · Approvals feel calm** | Queue & sharing | A single approvals lane with friendly copy and clear Approve / Hold buttons; connections drawer names who is online. | Queue service working, preview panel live, one-page operator guide with screenshots. |
| **G3 · Ops stay in control** | Dashboards & safeguards | Trial Control Center has four tiles (Systems, Memory, Approvals, Safety). Pause/rollback works, daily stand-up template in use. | Guardrail presets loaded, stop flow rehearsed, runbook printed or shared. |

When G0–G3 each take only a few minutes to verify, it is time to move from rehearsal to a real trial cohort.

## Trial flow at a glance

1. **Warm-up rehearsal** – Hit the preflight button, walk through a sample project like “Aurora Bikes Market Scan,” jot any confusing wording.
2. **Green-light chat** – Builder crew meets for 15 minutes, reads the gate checklist aloud, tags the release candidate, sets the trial date.
3. **Cohort A (internal)** – Send a welcome mail + first steps PDF, hold a five-minute daily stand-up to review dashboard tiles and approvals queue, collect notes in one shared doc.
4. **Cohort B (trusted partners)** – Share a lightweight kit (installer link, 10-minute video, emergency contact). Keep helpers in Guided mode until one full workflow finishes. Watch approvals wait times (<4 hours target).
5. **Wrap-up** – Celebrate wins, log friction, update the runbook and starter kit, tag the stable release, archive screenshots of the dashboard.

## Keeping the interface friendly

- One landing page with **Overview / Workflows / Safeguards** tabs; use the same names in training decks.
- “What’s in focus” appears in the Overview tab, the approvals card, and the Training Park so nobody wonders where facts came from.
- Alerts say things like “Needs a teammate’s OK” instead of protocol names, and every action in the queue pairs with a suggested decision.
- Recipe cards carry a picture, a one-line summary, and an optional “Explain as we go” toggle.
- Accessibility basics: large buttons, high contrast, keyboard shortcuts listed in a help overlay, screen-reader descriptions kept short.

Advanced toggles live behind an **Advanced** drawer so everyday users only see what they need.

## Autonomy later, not now

Fully autonomous helpers stay behind an extra gate (G4). Before we schedule that pilot we will:

- Publish the **Autonomy Lane Charter** (`docs/spec/autonomy_lane.md`) so everyone knows the sandbox rules.
- Finish the autonomy governor, kill switch, and rollback drills (tasks `trial-autonomy-governor`, `autonomy-rollback-playbook`).
- Add an autonomy tile to the Trial Control Center plus a quick “Stop now” button.
- Rehearse synthetic workloads twice (for example, a fake e-commerce shop) before inviting real users.

Until those are done we keep trials in guided mode.

## Builder checklist

(Internal note for the crew. Map each line to the backlog or task list.)

- Kernel triad work: `/actions`, `/state`, SQLite journal, hashed tokens.
- Memory fidelity: selector, compression, hygiene, context telemetry, Training Park dials.
- Approvals + sharing: staging queue, approvals lane, connections drawer, feedback readiness.
- Safety bundle: GTK/GLib upgrade, egress firewall presets, capsule guard, Prometheus tiles, red-team rehearsal.
- Automation helpers: `just triad-smoke`, `just context-ci`, `just trials-preflight`, `scripts/trials_preflight.sh`, `docs/ops/trial_runbook.md`, `ops/access_matrix.yaml`, `docs/ops/trials/`.
- Visual kit: home tabs, approvals lane, and dashboard mocks (task `trial-visual-kit`).
- Autonomy prep: tasks added (`trial-g0`–`trial-g3`, `trial-autonomy-governor`, `autonomy-lane-spec`, `autonomy-rollback-playbook`).

## Ready means…

- Anyone can tell in one glance if helpers are safe to keep running.
- Approvals feel like a short conversation, not a wall of logs.
- Dashboards say “Waiting approvals,” “Context freshness,” etc., and stay within targets.
- Pausing or rolling back feels as simple as locking a phone.
- Confidence is high enough that planning for autonomous pilots feels natural, not risky.

Once those statements stay true for a full trial cycle, we can welcome the next wave of users into Agent Hub.
