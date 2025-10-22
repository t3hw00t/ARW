---
title: Trial Runbook
---

# Trial Runbook

Updated: 2025-10-22
Type: Checklist (quick reference)

This runbook keeps our two-person trial routine lightweight. Use it with the Trial Readiness Plan, facilitator checklist, and quickstart note so we stay in sync without extra meetings.

## One-Time Setup

- Install the CLI once so logging helpers are available in every shell: `cargo install --path apps/arw-cli` (adds `arw-cli` to `~/.cargo/bin`).
- Optionally source `ops/context_watch.env.example` (or copy its exports into your shell profile) so `just context-watch` always targets the right base URL and log directory.

## Before the day starts

- Open the launcher Trial Control Center window (`Launcher → Trial Control`) and confirm Systems, Memory, Approvals, and Safety read “All good.” Record the numbers—including the Memory tile’s coverage gap and recall risk percentages—in a fresh copy of `docs/ops/trials/daily_log_template.md`. If you want an automated log, run `arw-cli context telemetry --watch --output docs/ops/trials/logs/context.log --output-rotate 10MB` during the session (stop with Ctrl+C when you’re done) or launch `just context-watch` to create per-day logs automatically. Adjust defaults with `ARW_CONTEXT_WATCH_BASE` / `ARW_CONTEXT_WATCH_OUTPUT_ROOT` / `ARW_CONTEXT_WATCH_OUTPUT_ROTATE` / `ARW_CONTEXT_WATCH_SESSION` when you point at remote hubs or alternate log folders, and use `just context-watch -- --date YYYY-MM-DD --session <slug>` if you need to recreate or split logs for earlier sessions. Pass `-- --rotate N` (>=64KB) / `-- --no-rotate` for one-off runs that need a different rollover policy.
- Run `just trials-preflight` (or click the preflight button in the Trial Control Center; it runs the helper and copies the CLI command if automation fails).
- Apply the trial guardrail preset with `just trials-guardrails preset=trial` (or `./scripts/trials_guardrails.sh --preset trial`). Check the Safety tile for the preset name and a fresh “applied …” timestamp before moving on.
- In the Trial Control Center, open the **Approvals lane** (see the [approvals guide](trials/approvals_lane_guide.md)), confirm your reviewer label with the **Set reviewer** button, and clear or assign any waiting items before we begin.
- Click **Connections** in the header to open the drawer and double-check the remote roster (it should just list the two of us during rehearsal). The drawer auto-refreshes, but glance at the “updated …” stamp to confirm the snapshot is current.
- Glance at the access matrix (ops/access_matrix.yaml) to verify tokens expiring today.

## During the day

- Keep helpers in Guided mode unless we both agree to flip on Autonomy Lane. If one of us is unsure, stay guided.
- Clear approvals after each major step (target: no cards waiting before we leave the session). The lane highlights who requested each action and how long it has been waiting, and the summary shows when the queue was last synced with the server.
- Keep an eye on the feedback delta log (`arw-cli events tail --kind feedback.delta` or `/admin/feedback` → `delta_log`) so we can spot surprise suggestions before they reach auto-apply. Note anything odd in the daily log.
- Use the Trial Control Center memory quarantine card for quick triage, and review the full queue once per block with `arw-cli admin review quarantine list --show-preview` (filter by state/source with `--state queued --source world_diff`). Use `--csv` or `--ndjson` when you need export-friendly snapshots. For deeper inspection run `arw-cli admin review quarantine show --id <entry>`, and resolve items in batches via `arw-cli admin review quarantine admit --id <entry> [--id ...] --decision admit|reject`; every admit call echoes the final entry so we can paste outcomes into the daily log.
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
5. Run `arw-cli smoke triad` (or `just triad-smoke`) to confirm the core service. Use `arw-cli smoke --help` if you need to tweak ports or keep the temp directory for forensic logs; the wrappers honor `SMOKE_TRIAD_TIMEOUT_SECS`/`SMOKE_TIMEOUT_SECS` and exit after 600 s by default (set to `0` for long investigations). They now build/run against debug artifacts for faster recovery—set `ARW_SMOKE_USE_RELEASE=1` when you explicitly need a release binary. Set `TRIAD_SMOKE_PERSONA` / `SMOKE_TRIAD_PERSONA` (or `ARW_PERSONA_ID`) before running if you want the synthetic action to tag a persona for telemetry dashboards. Point the harness at an already-running cluster by exporting `TRIAD_SMOKE_BASE_URL` (or passing `--base-url` to the CLI) so we talk to that URL instead of launching a temporary server. When `/healthz` needs authentication, export `TRIAD_SMOKE_HEALTHZ_BEARER` (or a raw `TRIAD_SMOKE_HEALTHZ_HEADER`) so the probe includes the right header; otherwise the script falls back to the admin token automatically. The same harness now supports `TRIAD_SMOKE_AUTH_MODE=basic` (add `TRIAD_SMOKE_BASIC_USER` / `TRIAD_SMOKE_BASIC_PASSWORD`), `TRIAD_SMOKE_AUTH_MODE=header` (pair with `TRIAD_SMOKE_AUTH_HEADER`), or mutual TLS via `TRIAD_SMOKE_TLS_CERT` / `TRIAD_SMOKE_TLS_KEY` / `TRIAD_SMOKE_TLS_CA`. The context smoke wrappers follow the persona flow—set SMOKE_CONTEXT_PERSONA (or reuse ARW_PERSONA_ID) if you want those checks to emit persona-tagged telemetry, and provide SMOKE_CONTEXT_BASE_URL / CONTEXT_SMOKE_BASE_URL when you are pointing at a remote hub.
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
