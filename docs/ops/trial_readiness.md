---
title: Trial Readiness Plan
---

# Trial Readiness Plan

Updated: 2025-09-26
Type: Playbook (quick guide)

Use this short guide to decide when Agent Hub feels good enough for **the two of us** to run a focused trial. We still reference the checklist (`docs/ops/trial_facilitator_checklist.md`) and onboarding note (`docs/ops/trials/onboarding_email_template.md`), but the scope stays intentionally small so we can iterate quickly before inviting anyone else.

## Four easy checkpoints

| Gate | Focus | What we should see | Quick prep for us |
| ---- | ----- | ------------------------ | ------------------------ |
| **G0 · Core feels steady** | Launch & status lights | Home opens quickly, status tray says “All good,” the self-check button passes. | Single activity history, refreshed sign-in tokens, preflight button wired. |
| **G1 · Memory makes sense** | Briefs & context | “What’s in focus” card shows key facts with timestamps and sources. | Summaries stay fresh, gentle “needs background” nudges appear, metrics hit the dashboard. |
| **G2 · Approvals feel calm** | Queue & sharing | A single approvals lane with friendly copy and clear Approve / Hold buttons; connections drawer names who is online. | Queue service working, preview panel live, one-page operator guide with screenshots. |
| **G3 · Ops stay in control** | Dashboards & safeguards | Trial Control Center has four tiles (Systems, Memory, Approvals, Safety). Pause/rollback works, daily stand-up template in use. | Guardrail presets loaded, stop flow rehearsed, runbook printed or shared. |

When G0–G3 each take only a few minutes to verify, it is time to move from rehearsal to a real trial cohort.

## Trial flow for two

1. **Warm-up rehearsal (10 min)** – Run the preflight button, walk through a sample task, and capture rough notes in the daily log.
2. **Go/No-Go sync (5 min)** – Quick call or chat: confirm the gate checklist, tag the release build, and pick the trial window.
3. **Hands-on window (30–60 min)** – One of us pilots the workflow while the other watches the dashboard, clears approvals, and jots highlights. Swap roles the next day to keep perspective fresh.
4. **Wrap-up (10 min)** – Screenshot the dashboard, summarize wins/friction in the log, and update the runbook/quickstart if anything felt clunky.

When this loop feels smooth we can think about inviting more teammates.

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

(Internal note for the two of us. We own every line for now, so keep it prioritized but lightweight by default.)

- Kernel triad work: `/actions`, `/state`, SQLite journal, hashed tokens.
- Memory fidelity: selector, compression, hygiene, context telemetry, Training Park dials.
- Approvals + sharing: staging queue, approvals lane, connections drawer, feedback readiness.
- Safety bundle: GTK/GLib upgrade, egress firewall presets, capsule guard, Prometheus tiles, red-team rehearsal.
- Automation helpers: `just triad-smoke`, `just context-ci`, `just trials-preflight`, `scripts/trials_preflight.sh`, `docs/ops/trial_runbook.md`, `ops/access_matrix.yaml`, `docs/ops/trials/`.
- Visual kit: home tabs, approvals lane, and dashboard mocks (task `trial-visual-kit`).
- Autonomy prep: tasks added (`trial-g0`–`trial-g3`, `trial-autonomy-governor`, `autonomy-lane-spec`, `autonomy-rollback-playbook`).

## Ready means…

- Either of us can tell in one glance if helpers are safe to keep running.
- Approvals feel like a short chat, not a wall of logs.
- Dashboards say “Waiting approvals,” “Context freshness,” etc., and stay within targets.
- Pausing or rolling back feels as simple as locking a phone.
- Confidence is high enough that planning for autonomous pilots feels natural, not risky.

Once those statements stay true for a full trial cycle, we can welcome the next wave of users into Agent Hub.
