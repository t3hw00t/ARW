---
title: Trial Runbook
---

# Trial Runbook

Updated: 2025-10-01
Type: Checklist (quick reference)

This runbook keeps our two-person trial routine lightweight. Use it with the Trial Readiness Plan, facilitator checklist, and quickstart note so we stay in sync without extra meetings.

## Before the day starts

- Open the launcher Trial Control Center window (`Launcher → Trial Control`) and confirm Systems, Memory, Approvals, and Safety read “All good.” Record the numbers in a fresh copy of `docs/ops/trials/daily_log_template.md`.
- Run `just trials-preflight` (or click the preflight button in the Trial Control Center; it runs the helper and copies the CLI command if automation fails).
- Apply the trial guardrail preset with `just trials-guardrails preset=trial` (or `./scripts/trials_guardrails.sh --preset trial`). Check the Safety tile for the preset name and a fresh “applied …” timestamp before moving on.
- In the Trial Control Center, open the **Approvals lane** (see the [approvals guide](trials/approvals_lane_guide.md)), confirm your reviewer label with the **Set reviewer** button, and clear or assign any waiting items before we begin.
- Click **Connections** in the header to open the drawer and double-check the remote roster (it should just list the two of us during rehearsal). The drawer auto-refreshes, but glance at the “updated …” stamp to confirm the snapshot is current.
- Glance at the access matrix (ops/access_matrix.yaml) to verify tokens expiring today.

## During the day

- Keep helpers in Guided mode unless we both agree to flip on Autonomy Lane. If one of us is unsure, stay guided.
- Clear approvals after each major step (target: no cards waiting before we leave the session). The lane highlights who requested each action and how long it has been waiting, and the summary shows when the queue was last synced with the server.
- If an alert appears (“Needs a teammate’s OK”), capture a quick note in the incident log and mention it in chat. Use the drawer to see who is connected before approving anything sensitive.
- Drop observations straight into the shared feedback doc; no extra survey needed while it’s just us.

## Daily stand-up template (see `docs/ops/trials/standup_template.md` for slide layout)

1. **Health** – Are all dashboard tiles green? Any slow starts?
2. **Approvals** – How many waiting items? Oldest age?
3. **Highlights** – Wins or surprises from helpers?
4. **Risks** – Anything we should pause or roll back?
5. **Next steps** – Actions, owners, due times.

## If something breaks

1. Pause helpers from the Trial Control Center (or kill switch) immediately.
2. If the run was under Autonomy Lane, jump to the [Autonomy rollback playbook](trials/autonomy_rollback_playbook.md) after pausing.
3. Reapply the guardrail preset (`just trials-guardrails preset=trial --dry_run=false`) when the stop is real; keep `--dry-run` for rehearsals so we can preview without touching the config file.
4. Capture the time and what people saw in the incident log.
5. Run `arw-cli smoke triad` (or `just triad-smoke`) to confirm the core service. Use `arw-cli smoke --help` if you need to tweak ports or keep the temp directory for forensic logs; the wrappers honor `SMOKE_TRIAD_TIMEOUT_SECS`/`SMOKE_TIMEOUT_SECS` and exit after 600 s by default (set to `0` for long investigations).
6. DM each other with the incident note so we decide fast.
7. Decide whether to resume, retry, or end the session.

## End-of-day wrap

- Clear or hand off approvals.
- Snapshot the dashboard tiles, save them in `docs/ops/trials/screenshots/` (add a short caption in the daily log), and log the filename in the daily log (see `docs/ops/trials/dashboard_snapshot.md`).
- Update the incident log and highlight the day’s wins. Log any “Budgets nearing limit” / “Budgets exhausted” alerts from the Autonomy tile so we can tune default budgets.
- Reapply the trial guardrail preset (`just trials-guardrails preset=trial`) if you rehearsed with `--dry-run`; the Safety tile should match the final recorded time.
- Check the access matrix for tokens or leases expiring overnight.

## Weekly review

- Compare dashboard snapshots for trends (approvals wait time, freshness dial, safety alerts).
- Revisit the Trial Readiness gates; confirm nothing regressed.
- Decide whether we run another pass tomorrow or pause for fixes.
- For autonomy prep: review progress on tasks `trial-autonomy-governor`, `autonomy-lane-spec`, `autonomy-rollback-playbook`.

## Contacts

Jot down how to reach each other quickly (phone + chat). That’s enough while it’s just us. If we add more people later, expand this section into a table again.
