---
title: Autonomy Lane Charter
---

# Autonomy Lane Charter

Updated: 2025-09-27
Type: Decision record
Status: Accepted

## Intent

Give trusted operators a predictable, high-signal way to let helpers run without manual approvals while preserving the calm, reversible guarantees of the core kernel. The Autonomy Lane defines the contract every autonomous run must satisfy: who can start it, how far it may reach, and how we stop or rewind it when something drifts.

## Definitions

- **Lane** — A named policy envelope (e.g., `trial-g4-autonomy`) that carries budgets, scope, telemetry, and audit labels.
- **Run** — A scheduled or ad-hoc autonomous session executing one or more recipes under the lane envelope.
- **Operator** — The human on call for the lane. They approve entry, monitor telemetry, and hold the stop switch.
- **Owner** — The product or project lead who signs off on lane scope and success criteria.

## Entry Criteria

1. **Scope approval** — Owner and operator record the allowed objectives, data sets, and autonomy window. We require dual sign-off before the scheduler can promote a run to `autonomous`.
2. **Budgets locked** — Time, token, and spend caps are resolved into the lane manifest. Cool-down triggers at 90% for any budget and flips the run back to Guided mode automatically.
3. **Guardrail presets** — Capsule guard leases, gating keys (`network:egress`, `runtime:manage`, `tools:high_privilege`), and egress posture (`public-only`, `partner`, or `custom`) must be staged and validated via the rehearsal checklist.
4. **Snapshots available** — Project and runtime snapshots must exist so the rollback recipe can complete in under two minutes.
5. **Telemetry sinks online** — `/state/episodes`, `/metrics`, and Trial Control Center overlays are reachable from the operator station; alerts go to the rotation channel.

## Lane Contract

- **Budgets** — The kernel enforces wall-clock, token, and spend budgets per run. Breaches emit `autonomy.budget.close_to_limit` events; hard stops emit `autonomy.budget.exhausted` followed by `autonomy.run.paused`. Update budgets through `POST /admin/autonomy/{lane}/budgets` (`dry_run:true` previews the change).
- **Destinations & I/O** — Destinations come from a manifest (`configs/autonomy/destinations.yaml`) that the lane references. DNS guard and the egress proxy enforce host/port limits; filesystem scope is restricted to the project workspace and declared mount points.
- **Runtime & tools** — Runs may claim runtimes tagged `autonomy_ready=true`. The orchestrator denies tools without `tool.contract.autonomy=true` metadata or missing safety notes. High-risk logic units must declare rollback hooks before they can execute autonomously.
- **Observation surface** — Every run streams a live ticker (objective, latest action, next planned step) and writes deltas to the shared event spine (`autonomy.tick.*`). Helpers document why decisions were taken via `world.belief` annotations.
- **Interruption guarantees** — Pause/stop commands preempt scheduler queues, revoke capability leases, and send an interrupt to active tools. Tool adapters must acknowledge within five seconds or get terminated by the supervisor.

## Operator Controls

- **Pause / Resume** — Trial Control Center exposes a single pause toggle wiring into `scheduler.pause_lane(lane_id)`. Resume requires operator authentication plus reason logging.
- **Stop & Snapshot** — `Stop` flushes outstanding jobs. Operators capture a project snapshot manually (see the Autonomy Rollback Playbook) and record an `autonomy.run.stopped` event with the operator ID until the dedicated endpoint returns.
- **Stop & Snapshot** — `Stop` flushes outstanding jobs. Operators capture a fresh snapshot via `POST /projects/{proj}/snapshot` and record an `autonomy.run.stopped` event with the operator ID.
- **Rollback** — Control bar links directly to the Runbook (see [ops/trials/autonomy_rollback_playbook.md](../ops/trials/autonomy_rollback_playbook.md)). Operators can invoke the automated recipe or the manual checklist.
- **Escalation** — Pager channel receives structured alerts (`autonomy.alert.*`). Playbook lists escalation tree (primary operator → owner → security liaison).

## Telemetry & Audits

- Emit structured events: `autonomy.run.started`, `.paused`, `.resumed`, `.stopped`, `.rollback.started/completed`, `.budget.*`, `.egress.blocked`, `.tool.denied`.
- `/state/autonomy/lanes` read-model summarizes current runs, budget headroom, outstanding alerts, and last operator action.
- Prometheus counters: `autonomy_runs_total{status}` and `autonomy_interrupts_total{reason}`. Gauges capture `autonomy_budget_remaining_seconds` and `autonomy_budget_remaining_tokens`.
- Trial dossier receives a run summary snapshot (`ops/trials/README.md` guidance) after each autonomous session.

## Rollback & Recovery

The lane never runs without a fresh rollback rehearsal. The two-minute recipe lives in [Autonomy rollback playbook](../ops/trials/autonomy_rollback_playbook.md) and includes:
- Identifying the last good snapshot (project, runtime, and guardrail presets).
- Reverting CAS-stored configs via the patch engine.
- Validating the helper is back in Guided mode before clearing the alert.

## Implementation Checklist

- [x] Charter published and accepted.
- [x] `trial-autonomy-governor`: wire scheduler kill switch, pause lane API, Trial Control Center controls.
- [x] `autonomy-rollback-playbook`: keep runbook current; rehearsal tracked per cohort.
- [ ] `autonomy-lane-spec`: keep this ADR aligned with implementation; revisit quarterly.
- [ ] `trial-g3` / `trial-g4`: update gate criteria once kill switch and rollback rehearsals ship.

## Change History

- 2025-09-29 — Implemented autonomy kill switch registry, APIs, and Trial Control Center controls.
- 2025-09-26 — Charter promoted from draft to accepted; added lane contract, telemetry, and rollback alignment.
- 2025-09-18 — Initial draft capturing intent and guardrail checklist.

## Open Questions

1. Should we gate lane entry on two-person sign-off at runtime (launcher prompt) or settle for the pre-run checklist?
2. Do we expose a confidence score or anomaly meter to end users, or keep it operator-only to avoid alert fatigue?
3. What is the default exhaust behavior after multiple rollbacks (pause indefinitely vs. allow retries)?
