---
title: Autonomy Rollback Playbook
---

# Autonomy Rollback Playbook

Updated: 2025-09-27
Type: Runbook
Status: Active

## Purpose

Return an autonomous lane to a known-good guided state in two minutes or less. Operators follow this playbook whenever a helper misbehaves, budgets spike, or a manual stop is requested during Gate G4 rehearsals or production trials.

## Preflight (keep current)

- Latest project snapshot captured via `POST /projects/{proj}/snapshot` within the last 30 minutes.
- Runtime profile snapshot (launcher Runtime Manager → `Save profile snapshot`).
- Guardrail preset bundle exported (`configs/gating.toml`, `configs/trust_capsules.json`).
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

When passing options that start with `-`, insert `--` after the `just` target (example: `just autonomy-rollback -- --dry-run lane=<lane_id>`).

What the helper attempts:

1. Pause the lane (`POST /admin/autonomy/{lane}/pause`).
2. Flush in-flight and queued jobs (`DELETE /admin/autonomy/{lane}/jobs`).
3. Discover lane metadata and note the most recent snapshot id surfaced by `/state/autonomy/lanes/{lane}`.
4. Capture a fresh snapshot with `POST /projects/{proj}/snapshot`.
5. Request a runtime restore via `POST /orchestrator/runtimes/{id}/restore` (falls back to Launcher Runtime Manager when unavailable).
6. Reapply guardrail presets using `POST /policy/guardrails/apply` (`dry_run:true` for rehearsal).

When an endpoint is missing (early builds) the script prints a WARN line with the equivalent manual curl invocation so the operator can finish the step by hand. Pair it with the incident note in the template below.

## Manual Checklist (fallback)

1. **Pause immediately**
   - Hit Pause in Trial Control Center → confirm alert note (include reason).
2. **Cut automation**
   - `DELETE /admin/autonomy/{lane_id}/jobs?state=in_flight`.
   - `POST /policy/guardrails/apply` with the safety preset (`dry_run:true` when rehearsing).
3. **Restore project state**
   - Use `/state/projects` to confirm the current file tree.
   - `POST /projects/{proj}/snapshots/{snapshot}/restore` to rewind, or follow the manual replay checklist.
   - Record the snapshot id used in the incident log.
4. **Restore runtime state**
   - `POST /orchestrator/runtimes/{runtime}/restore` to trigger the automation hook.
   - Launcher Runtime Manager → select snapshot → `Restore and restart` if automation is unavailable.
   - Verify `/state/runtime_supervisor` shows `ready` with expected profile tag.
5. **Reapply guardrails**
   - `POST /policy/guardrails/apply` with the saved preset (`dry_run:true` permitted for rehearsal).
   - Confirm `capsule.guard.state=healthy` in `/state/policy/capsules`.
6. **Validate**
   - Ensure `/state/autonomy/lanes/:lane_id` reports `mode="guided"` and zero active jobs.
   - Run smoke task: `POST /actions` with `kind="demo.echo"` inside the project to confirm helpers respond.
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
- Budget tuning: `POST /admin/autonomy/{lane}/budgets` (use `dry_run:true` to preview).
- Event watch: `/events?kind=autonomy.*`
