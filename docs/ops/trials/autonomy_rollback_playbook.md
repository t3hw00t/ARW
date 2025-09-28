---
title: Autonomy Rollback Playbook
---

# Autonomy Rollback Playbook

Updated: 2025-09-26
Type: Runbook
Status: Active

## Purpose

Return an autonomous lane to a known-good guided state in two minutes or less. Operators follow this playbook whenever a helper misbehaves, budgets spike, or a manual stop is requested during Gate G4 rehearsals or production trials.

## Preflight (keep current)

- Latest project snapshot (`/admin/projects/:id/snapshot`) stored with timestamp < 30 min.
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
4. Restore the project and runtime using the unified helpers (see manual checklist below until the API is fully automated).
5. Reapply guardrail presets. (Until the admin event publish helper returns, log the rollback in the incident template instead.)

When an endpoint is missing (early builds) the script prints a WARN line with the equivalent manual curl invocation so the operator can finish the step by hand. Pair it with the incident note in the template below.

## Manual Checklist (fallback)

1. **Pause immediately**
   - Hit Pause in Trial Control Center → confirm alert note (include reason).
2. **Cut automation**
   - `DELETE /admin/autonomy/{lane_id}/jobs?state=in_flight`.
   - Revoke leases: `POST /admin/capabilities/revoke` for lane scope.
3. **Restore project state**
   - Use `/state/projects` to confirm the current file tree.
   - Run the project restore helper (planned unified endpoint) or follow the ops handbook to replay the snapshot from shared storage.
   - Record the snapshot id used in the incident log.
4. **Restore runtime state**
   - Launcher Runtime Manager → select snapshot → `Restore and restart`.
   - Verify `/state/runtimes` shows `ready` with expected profile tag.
5. **Reapply guardrails**
   - `PATCH /admin/gating` with autonomy preset (stored in `docs/ops/trials/README.md` dossier).
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
- Budget tuning: `/admin/autonomy/{lane_id}/budgets`
- Event watch: `/events?kind=autonomy.*`
