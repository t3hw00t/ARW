---
title: Trial Runbook
---

# Trial Runbook

Updated: 2025-09-26
Type: Checklist (quick reference)

This runbook keeps our two-person trial routine lightweight. Use it with the Trial Readiness Plan, facilitator checklist, and quickstart note so we stay in sync without extra meetings.

## Before the day starts

- Open the Trial Control Center (or `/admin/debug` dashboards) and confirm Systems, Memory, Approvals, and Safety are green. Record the numbers in a fresh copy of `docs/ops/trials/daily_log_template.md`.
- Run `just trials-preflight` (or press the Home screen preflight button when it ships).
- Check the approvals lane is empty; if not, ping the other person before starting.
- Glance at the access matrix (ops/access_matrix.yaml) to verify tokens expiring today.

## During the day

- Keep helpers in Guided mode unless we both agree to flip on Autonomy Lane. If one of us is unsure, stay guided.
- Review approvals after each major step (target: decisions cleared before we leave the session).
- If an alert appears (“Needs a teammate’s OK”), capture a quick note in the incident log and mention it in chat.
- Drop observations straight into the shared feedback doc; no extra survey needed while it’s just us.

## Daily stand-up template (see `docs/ops/trials/standup_template.md` for slide layout)

1. **Health** – Are all dashboard tiles green? Any slow starts?
2. **Approvals** – How many waiting items? Oldest age?
3. **Highlights** – Wins or surprises from helpers?
4. **Risks** – Anything we should pause or roll back?
5. **Next steps** – Actions, owners, due times.

## If something breaks

1. Pause helpers from the Trial Control Center (or kill switch) immediately.
2. Capture the time and what people saw in the incident log.
3. Run `just triad-smoke` to confirm the core service.
4. DM each other with the incident note so we decide fast.
5. Decide whether to resume, retry, or end the session.

## End-of-day wrap

- Clear or hand off approvals.
- Snapshot the dashboard tiles, save them in `docs/ops/trials/screenshots/` (add a short caption in the daily log), and log the filename in the daily log (see `docs/ops/trials/dashboard_snapshot.md`).
- Update the incident log and highlight the day’s wins.
- Check the access matrix for tokens or leases expiring overnight.

## Weekly review

- Compare dashboard snapshots for trends (approvals wait time, freshness dial, safety alerts).
- Revisit the Trial Readiness gates; confirm nothing regressed.
- Decide whether we run another pass tomorrow or pause for fixes.
- For autonomy prep: review progress on tasks `trial-autonomy-governor`, `autonomy-lane-spec`, `autonomy-rollback-playbook`.

## Contacts

Jot down how to reach each other quickly (phone + chat). That’s enough while it’s just us. If we add more people later, expand this section into a table again.
