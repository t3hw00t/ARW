---
title: Autonomy Rollback Playbook
---

# Autonomy Rollback Playbook

Updated: 2025-10-11
Type: Runbook
Status: Active

## Purpose

Return an autonomous lane to a known-good guided state in two minutes or less. Operators follow this playbook whenever a helper misbehaves, budgets spike, or a manual stop is requested during Gate G4 rehearsals or production trials.

## Preflight (keep current)

- Latest project snapshot captured via `POST /projects/{proj}/snapshot` within the last 30 minutes.
- Runtime profile snapshot (launcher Runtime Manager → `Save profile snapshot`).
- Guardrail preset bundle exported (`configs/gating.toml`, `configs/trust_capsules.json`) and the trial preset available at `configs/guardrails/trial.toml`.
- Operator console open to Trial Control Center (pause/stop visible).
- Pager channel bookmark ready for status updates.

## Quick Script

Run the helper when you have an admin token handy:

```
ARW_ADMIN_TOKEN=<token> just autonomy-rollback lane=<lane_id> project=<project_id>
```

Optional flags:

- `runtime=<runtime_id>` — restart a managed runtime profile after the restore.
- `snapshot=<snapshot_id>` — restore a specific project snapshot (auto-selects the freshest one when omitted and the endpoint supports it).
- `guardrails=<preset>` — reapply a named gating preset.
- `base=<url>` — point at a different server origin.
- `--dry-run` — print the planned API calls without mutating anything.

When passing options that start with `-`, insert `--` after the `just` target (example: `just autonomy-rollback -- --dry-run lane=<lane_id>`). For quick guardrail checks outside a full rollback, use `./scripts/trials_guardrails.sh --preset trial` (add `--dry-run` during rehearsal).

What the helper attempts:

1. Stop the lane (`POST /admin/autonomy/{lane}/stop` or `arw-cli admin autonomy stop --lane <lane>`). Fallback for older builds: `POST /admin/autonomy/{lane}/pause` followed by `DELETE /admin/autonomy/{lane}/jobs`.
2. Discover lane metadata and note the most recent snapshot id surfaced by `/state/autonomy/lanes/{lane}` (the helper falls back to `arw-cli admin autonomy lane --lane <lane>` when running manually).
3. Capture a fresh snapshot with `POST /projects/{proj}/snapshot`.
4. Request a runtime restore via `POST /orchestrator/runtimes/{id}/restore` (falls back to Launcher Runtime Manager when unavailable).
5. Reapply guardrail presets using `POST /policy/guardrails/apply` (`dry_run:true` for rehearsal). The helper shells out to the same logic exposed via `scripts/trials_guardrails.sh`.

When an endpoint is missing (early builds) the script prints a WARN line with the equivalent manual curl invocation so the operator can finish the step by hand. Pair it with the incident note in the template below.

## Manual Checklist (fallback)

1. **Pause immediately**
   - Hit Stop in Trial Control Center (sends `POST /admin/autonomy/{lane}/stop`) or run `arw-cli admin autonomy stop --lane <lane_id> [--operator you] [--reason text]` (the CLI records operator + reason for the audit trail).
   - Fallback: Pause first (`arw-cli admin autonomy pause --lane <lane_id>`) and then flush in-flight jobs with `arw-cli admin autonomy flush --lane <lane_id> --state in_flight`. Older builds still accept the raw HTTP calls (`POST /admin/autonomy/{lane}/pause` + `DELETE /admin/autonomy/{lane}/jobs?state=in_flight`).
2. **Cut automation**
   - `POST /policy/guardrails/apply` with the safety preset (`dry_run:true` when rehearsing).
3. **Restore project state**
   - Use `/state/projects` to confirm the current file tree.
   - `POST /projects/{proj}/snapshots/{snapshot}/restore` to rewind, or follow the manual replay checklist.
   - Record the snapshot id used in the incident log.
4. **Restore runtime state**
   - `POST /orchestrator/runtimes/{runtime}/restore` to trigger the automation hook.
   - `arw-cli runtime restore --id {runtime}` offers the same call from the terminal and reports whether the restart budget would block the request.
   - If the call returns `429 Too Many Requests`, the restart budget window is exhausted; wait for the reset timestamp in the payload or widen the limit via `ARW_RUNTIME_RESTART_MAX` / `ARW_RUNTIME_RESTART_WINDOW_SEC` before retrying.
   - Launcher Runtime Manager → select snapshot → `Restore and restart` if automation is unavailable.
   - Verify `/state/runtime_supervisor` shows `ready` with expected profile tag.
5. **Reapply guardrails**
   - `POST /policy/guardrails/apply` with the saved preset (`dry_run:true` permitted for rehearsal) or run `./scripts/trials_guardrails.sh --preset trial` for the default trial profile.
   - Confirm `capsule.guard.state=healthy` in `/state/policy/capsules`.
6. **Validate**
   - Ensure `/state/autonomy/lanes/:lane_id` (or `arw-cli admin autonomy lane --lane <lane_id>`) reports `mode="guided"` and zero active jobs.
   - Run smoke task: `POST /actions` with `kind="demo.echo"` inside the project to confirm helpers respond.
   - Check `/metrics` for `arw_autonomy_interrupts_total{reason="pause"}` and the relevant `stop_flush_*` counters to confirm the kill switch path registered.
7. **Communicate**
   - Post status update in trial channel (template below).
   - Log incident entry in `docs/ops/trials/daily_log_<date>.md`.

## Communication Template

```
:rotating_light: Autonomy lane rolled back to guided.
Lane: {lane_id}
Triggered by: {operator}
Reason: {brief description}
Snapshot: {timestamp}
Follow-up: {next steps or "monitoring"}
```

## Post-Rollback Tasks

- File a short incident card (owner + operator) noting root cause, time to recover, and whether automation may resume.
- Schedule a rehearsal if the quick script or manual checklist drifted.
- Update the Trial Dossier with screenshots and event IDs for audit.

## References

- Charter: [spec/autonomy_lane.md](../../spec/autonomy_lane.md)
- Trial dossier guidance: [ops/trials/README.md](README.md)
- Budget tuning: `arw-cli admin autonomy budgets --lane <lane_id> [--wall-clock-secs N] [--tokens N] [--spend-cents N] [--dry-run]` (falls back to `POST /admin/autonomy/{lane}/budgets` when the CLI is unavailable).
- Event watch: `/events?kind=autonomy.*`
