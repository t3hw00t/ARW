---
title: Trial Facilitator Checklist
---

# Trial Facilitator Checklist

Updated: 2025-09-26
Type: One-pager

Use this sheet to launch and run a trial with minimal prep. Tick items as you go; link to the detailed docs if you need more depth.

## Prep (week before)
- [ ] Pick Gate (G0–G3) status from the Trial Readiness Plan.
- [ ] Fill in the access matrix (`ops/access_matrix.yaml`).
- [ ] Update the Trial Runbook contacts section.
- [ ] Clone a daily log template into `docs/ops/trials/` for each day of the trial.
- [ ] Draft the stand-up deck using `docs/ops/trials/standup_template.md`.
- [ ] Customize the participant quickstart (`docs/ops/trial_quickstart.md`) with links and contacts.
- [ ] Confirm `just trials-preflight` runs clean on staging.

## Kickoff (day 0)
- [ ] Send onboarding email (see template) + quickstart PDF.
- [ ] Distribute tokens and note expiry dates in the access matrix.
- [ ] Schedule daily stand-up and share the agenda.
- [ ] Verify Trial Control Center tiles are green and screenshot the baseline.

## Daily loop
- [ ] Run preflight each morning (`just trials-preflight`).
- [ ] Capture dashboard snapshot and paste metrics into the daily log.
- [ ] Hold the stand-up; update highlights/risks in the log.
- [ ] Review approvals every two hours; record any >4h waits.
- [ ] Log incidents with time, action taken, and follow-up.
- [ ] Collect participant feedback (in-app or shared doc).

## Wrap-up (final day)
- [ ] Close outstanding approvals or hand off.
- [ ] Archive dashboard snapshots and daily logs in the dossier.
- [ ] Compile a short summary (wins, friction, metrics) for leadership.
- [ ] Decide on next phase: expand, repeat, or prepare G4 autonomy rehearsal.

Keep this checklist in the trial dossier for quick reference.
