# Trial Dossier

Updated: 2025-10-09
Type: How‑to

Use this folder to store lightweight records during the trial. Keep files short and human-friendly so anyone joining mid-stream can catch up.

## What to capture

- Daily stand-up notes (one row per day).
- Incident or pause summaries (what happened, who responded, next steps).
- Dashboard snapshots (PNG exports or links) with the date in the filename.
- Autonomy rehearsals once Gate G4 preparation starts.

## Templates

- `daily_log_template.md` — copy once per day for Cohort A/B.
- `dashboard_snapshot.md` — jot the numbers you expect to see when saving a screenshot.
- `autonomy_rollback_playbook.md` — step-by-step rollback instructions when the Autonomy Lane is active.
- `approvals_lane_guide.md` — illustrated walkthrough for Gate G2 (approvals lane + connections drawer).

Store sensitive data elsewhere; this dossier is for coordination only.

## Helpers

- `scripts/trials_guardrails.sh` — applies the trial guardrail preset; pass `--dry-run` during rehearsal and run without it when locking in the day’s config.
- `scripts/autonomy_rollback.sh` — autonomy lane rollback helper (see `autonomy_rollback_playbook.md`).
