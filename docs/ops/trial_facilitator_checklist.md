---
title: Trial Facilitator Checklist
---

# Trial Facilitator Checklist

Updated: 2025-10-09
Type: One-pager

Use this sheet to launch and run our two-person trial. Tick items as you go; if an item feels heavy for the two of us, make a quick note and move on.

## Prep (day before)
- [ ] Pick Gate (G0–G3) status from the Trial Readiness Plan.
- [ ] Fill in the access matrix (`ops/access_matrix.yaml`) with both of our machines/tokens.
- [ ] Update the Trial Runbook contacts note with how to reach each other fast.
- [ ] Clone a daily log template into `docs/ops/trials/` for each day of the trial.
- [ ] If we want visuals, duplicate the stand-up template (`docs/ops/trials/standup_template.md`). Otherwise plan a quick chat.
- [ ] Customize the participant quickstart (`docs/ops/trial_quickstart.md`) so it reflects our current links.
- [ ] Confirm `just trials-preflight` runs clean on staging.

## Kickoff (day 0)
- [ ] Send the onboarding note (or DM) with the quickstart link.
- [ ] Drop tokens in our shared vault and copy expiry dates into the access matrix.
- [ ] Schedule a quick daily sync (video or chat).
- [ ] Verify Trial Control Center (or `/admin/debug`) tiles are green and screenshot the baseline.

## Daily loop
- [ ] Run preflight each morning (`just trials-preflight`).
- [ ] Capture dashboard snapshot and paste metrics into the daily log.
- [ ] Hold the stand-up; update highlights/risks in the log.
- [ ] Review approvals during/after the session; record anything that sat too long.
- [ ] Log incidents with time, action taken, and follow-up.
- [ ] Add feedback straight to the shared doc or the daily log.

## Wrap-up (final day)
- [ ] Close outstanding approvals or hand off.
- [ ] Archive dashboard snapshots and daily logs in the dossier.
- [ ] Compile a short summary (wins, friction, metrics) for ourselves and future notes.
- [ ] Decide whether we repeat, pause for fixes, or tee up the next experiment (e.g., G4 prep).

Keep this checklist in the trial dossier for quick reference.
