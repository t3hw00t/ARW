---
title: Trial Runbook
---

# Trial Runbook

Updated: 2025-09-26
Type: Checklist (quick reference)

This runbook keeps the daily trial routine lightweight. Use it side-by-side with the Trial Readiness Plan.

## Before the day starts

- Open the Trial Control Center; confirm all four tiles (Systems, Memory, Approvals, Safety) are green.
- Run `just trials-preflight` or press the Home screen preflight button.
- Check the approvals lane is empty; if not, page the on-call approver.
- Glance at the access matrix (ops/access_matrix.yaml) to verify tokens expiring today.

## During the day

- Keep helpers in Guided mode unless the team explicitly opts into Autonomy Lane.
- Review approvals every two hours (target: decisions < 4 hours old).
- If an alert appears (“Needs a teammate’s OK”), capture a quick note in the incident log.
- Encourage trial participants to drop observations into the shared feedback doc or use the in-app survey.

## Daily stand-up template

1. **Health** – Are all dashboard tiles green? Any slow starts?
2. **Approvals** – How many waiting items? Oldest age?
3. **Highlights** – Wins or surprises from helpers?
4. **Risks** – Anything we should pause or roll back?
5. **Next steps** – Actions, owners, due times.

## If something breaks

1. Pause helpers from the Trial Control Center.
2. Capture the time and what people saw in the incident log.
3. Run `just triad-smoke` to confirm the core service.
4. Reach out to the builder crew channel with the incident note.
5. Decide whether to resume, retry, or end the session.

## End-of-day wrap

- Clear or hand off approvals.
- Snapshot the dashboard tiles (screenshots or export) and store them in the trial dossier.
- Update the incident log and highlight the day’s wins.
- Check the access matrix for tokens or leases expiring overnight.

## Weekly review

- Compare dashboard snapshots for trends (approvals wait time, freshness dial, safety alerts).
- Revisit the Trial Readiness gates; confirm nothing regressed.
- Decide whether to expand, pause, or adjust the cohort.
- For autonomy prep: review progress on tasks `trial-autonomy-governor`, `autonomy-lane-spec`, `autonomy-rollback-playbook`.

## Contacts

| Role | Name | Contact |
| ---- | ---- | ------- |
| Ops lead | _fill in_ | _email/phone_ |
| Builder on call | _fill in_ | _email/phone_ |
| Approvals buddy | _fill in_ | _email/chat_ |
| Comms/PR | _fill in_ | _email/chat_ |

Keep this sheet short; update names and contacts as rotations change.
