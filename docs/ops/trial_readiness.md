---
title: Trial Readiness Plan
---

# Trial Readiness Plan

Updated: 2025-10-01
Type: Playbook (quick guide)

Use this short guide to decide when Agent Hub feels good enough for **the two of us** to run a focused trial. Keep [the facilitator checklist](trial_facilitator_checklist.md) and [onboarding note](trials/onboarding_email_template.md) nearby—the scope stays intentionally small so we can iterate quickly before inviting anyone else.

## Four easy checkpoints

| Gate | Focus | What we should see | Quick prep for us |
| ---- | ----- | ------------------------ | ------------------------ |
| **G0 · Core feels steady** | Launch & status lights | Home opens quickly, status tray says “All good,” scripted preflight checks pass. | Run `just triad-smoke` (hits `/actions`, `/state/projects`, `/events` resume; exits after `SMOKE_TRIAD_TIMEOUT_SECS`/`SMOKE_TIMEOUT_SECS`, default 600 s), run `just trials-preflight` or the launcher preflight button, verify the Project Hub Home surface loads, refresh sign-in tokens, document the preflight outcome in the log. |
| **G1 · Memory makes sense** | Briefs & context | “What’s in focus” card (or Project Hub Activity panel) shows key facts with timestamps and sources. | Run `just context-ci`, confirm “needs background” nudges render in the activity view, and verify the memory metrics tile in `/admin/debug` updates. |
| **G2 · Approvals feel calm** | Queue & sharing | Approvals drawer (launcher) or `/admin/debug` queue shows clear Approve / Hold actions; connections drawer names who is online. | Queue service working, preview panel live, latest screenshots stored in `docs/ops/trials/screenshots/` with filenames logged per [Trial Runbook](trial_runbook.md); see the [approvals guide](trials/approvals_lane_guide.md) for daily flow. |
| **G3 · Ops stay in control** | Dashboards & safeguards | Trial Control Center launcher window (see `docs/design/trial_visual_kit.md`) or `/admin/debug` tiles cover Systems, Memory, Approvals, Safety. Approvals/Connections/Autonomy cards auto-refresh with “updated …” stamps so drift is visible, pause/rollback flow rehearsed, daily stand-up template in use. | Guardrail preset `configs/guardrails/trial.toml` applied via `just trials-guardrails` (Safety tile shows the latest stamp), stop flow rehearsed with `scripts/autonomy_rollback.sh --dry-run`, runbook printed or shared, notes capture how to reach the dashboard today. |

G4 expands the Trial Control Center with an Autonomy tile once we open that gate; until then we lean on the four-tile dashboard pattern (mock or live) described in the visual kit.

When G0–G3 each take only a few minutes to verify, it is time to move from rehearsal to a real trial cohort.

## Trial flow for two

1. **Warm-up rehearsal (10 min)** – Run `just trials-preflight` (or the in-product preflight button when it ships), walk through a sample task, and capture rough notes in the daily log ([daily_log_template.md](trials/daily_log_template.md)).
2. **Go/No-Go sync (5 min)** – Quick call or chat: confirm the gate checklist, tag the release build, and pick the trial window.
3. **Hands-on window (30–60 min)** – One of us pilots the workflow while the other watches the dashboard, clears approvals, and jots highlights. Swap roles the next day to keep perspective fresh.
4. **Wrap-up (10 min)** – Screenshot the dashboard, summarize wins/friction in the log, and update the runbook/quickstart ([trial_runbook.md](trial_runbook.md), [trial_quickstart.md](trial_quickstart.md)) if anything felt clunky.

When this loop feels smooth we can think about inviting more teammates.

## Keeping the interface friendly

- One landing page with **Overview / Workflows / Safeguards** tabs; use the same names in training decks (see `docs/design/trial_visual_kit.md` for the mock + implementation guidance).
- The "What's in focus" card now includes a See sources button that opens `/admin/debug#memory` so we can jump from summary to evidence quickly, and it shows both relative and absolute freshness for accessibility.
- “What’s in focus” appears in the Overview tab, the approvals card, and the Training Park so nobody wonders where facts came from. If the launcher preview isn’t available, surface the same facts through the Project Hub Activity panel.
- Alerts say things like “Needs a teammate’s OK” instead of protocol names, and every action in the queue pairs with a suggested decision.
- The Trial Control Center header now includes a **Connections** button—click it to open a lightweight drawer that lists who is online before you approve or hand off work.
- Recipe cards carry a picture, a one-line summary, and an optional “Explain as we go” toggle; treat the visual kit as the source of truth until the launcher panel lands.
- Accessibility basics: large buttons, high contrast, keyboard shortcuts listed in a help overlay, screen-reader descriptions kept short.

Advanced toggles live behind an **Advanced** drawer so everyday users only see what they need.

## Autonomy later, not now

Fully autonomous helpers stay behind an extra gate (G4). Before we schedule that pilot we will:

- Publish the **Autonomy Lane Charter** ([spec/autonomy_lane.md](../spec/autonomy_lane.md)) so everyone knows the sandbox rules.
- Finish the autonomy governor, kill switch, and rollback drills (tasks `trial-autonomy-governor`, `autonomy-rollback-playbook`).
- Rehearse the automation helper: `ARW_ADMIN_TOKEN=<token> just autonomy-rollback lane=<lane_id> project=<project_id>`; log results in the trial dossier.
- Trial Control Center includes the Autonomy tile with Pause/Resume/Flush controls for the kill switch drill.
- Rehearse synthetic workloads twice (for example, a fake e-commerce shop) before inviting real users.

Until those are done we keep trials in guided mode.

## Builder checklist

(Internal note for the two of us. We own every line for now, so keep it prioritized but lightweight by default.)

- Kernel triad work: `/actions`, `/state`, SQLite journal, hashed tokens.
- Memory fidelity: selector, compression, hygiene, context telemetry, Training Park dials.
- Approvals + sharing: staging queue, approvals lane, connections drawer, feedback readiness.
- Safety bundle: GTK/GLib upgrade, egress firewall presets, capsule guard, Prometheus tiles, red-team rehearsal.
- Automation helpers: `arw-cli smoke triad`, `arw-cli smoke context`, `just triad-smoke`, `just context-ci`, `just trials-preflight`, `just trials-guardrails`, `scripts/smoke_triad.(ps1|sh)`, `scripts/smoke_context.(ps1|sh)` — all honor `SMOKE_TIMEOUT_SECS` (script-specific envs override) so stalled runs stop after 600 s unless you set the knobs to `0` — plus `scripts/trials_preflight.ps1`/`scripts/trials_preflight.sh`, `scripts/trials_guardrails.sh`, [Trial Runbook](trial_runbook.md), `ops/access_matrix.yaml`, [`docs/ops/trials/`](trials/README.md).
- Visual kit: home tabs, approvals lane, and dashboard mocks (task `trial-visual-kit`).
- Autonomy prep: tasks added (`trial-g0`–`trial-g3`, `trial-autonomy-governor`, `autonomy-lane-spec`, `autonomy-rollback-playbook`).

## Ready means…

- Either of us can tell in one glance if helpers are safe to keep running.
- Approvals feel like a short chat, not a wall of logs.
- Dashboards say “Waiting approvals,” “Context freshness,” etc., and stay within targets.
- Pausing or rolling back feels as simple as locking a phone.
- Confidence is high enough that planning for autonomous pilots feels natural, not risky.

Once those statements stay true for a full trial cycle, we can welcome the next wave of users into Agent Hub.
