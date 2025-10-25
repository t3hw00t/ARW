---
title: Maintenance & Scheduling
---

# Maintenance & Scheduling

Updated: 2025-10-25
Type: How-to

Keep ARW hubs healthy by running the bundled maintenance scripts on a regular cadence. This handbook describes the default task set, configuration knobs, and scheduling recipes for Linux (cron/systemd) and Windows (Task Scheduler).

## Default Sweep

Two entrypoints ship with the repo and perform the same actions:

- `scripts/maintenance.sh` — POSIX shell runner.
- `scripts/maintenance.ps1` — PowerShell 7+ runner.

Without arguments they execute:

```
clean -> prune-logs -> prune-tokens -> docs -> cargo-check -> audit-summary -> pointer-migrate
```

Common flags (names match the shell/PowerShell variants):

- `--state-dir DIR` / `-StateDir DIR` — override the state directory (default `apps/arw-server/state`).
- `--pointer-consent private|shared|public` / `-PointerConsent` — consent level applied when pointers lack metadata (default `private`).
- `--dry-run` / `-DryRun` — preview actions without mutating state.

Linux example:

```bash
# Preview the sweep against the production state directory
scripts/maintenance.sh --dry-run --state-dir /var/arw/state

# Apply updates (stop arw-server first if it owns the target state dir)
scripts/maintenance.sh --state-dir /var/arw/state --pointer-consent shared
```

Windows example (PowerShell 7+):

```powershell
# Preview the sweep
pwsh -NoProfile -File scripts\maintenance.ps1 -DryRun `
  -StateDir 'C:\ARW\apps\arw-server\state'

# Apply updates (stop the service first if this host runs arw-server)
pwsh -NoProfile -File scripts\maintenance.ps1 `
  -StateDir 'C:\ARW\apps\arw-server\state' `
  -PointerConsent Shared
```

> **Stopping the server:** pointer canonicalisation opens each SQLite database for write access. Stop `arw-server` (or ensure the state directory is unused) before running a non-dry-run sweep.
>
> **Helper wrapper:** `ops/windows/invoke-maintenance.ps1` stops/starts the `arw-server` service around the maintenance call. Edit the configuration block at the top (`$RepoRoot`, `$StateDir`, `$ServiceName`, `$PointerConsent`) before using it interactively or from Task Scheduler.

## Scheduling with cron

Create `/etc/cron.weekly/arw-maintenance` (adjust cadence to your needs):

```bash
#!/usr/bin/env bash
set -euo pipefail
cd /opt/arw

# Stop the server if this host runs the service
systemctl stop arw-server.service

# Run the sweep; log output to /var/log
scripts/maintenance.sh --state-dir /var/arw/state --pointer-consent shared \
  >> /var/log/arw-maintenance.log 2>&1

systemctl start arw-server.service
```

Make it executable: `chmod +x /etc/cron.weekly/arw-maintenance`.

> Template: copy `ops/cron/arw-maintenance.sh` into `/etc/cron.weekly/`, edit the environment variables at the top (paths, consent level, target service), and mark it executable.

## Scheduling with systemd timers

Drop the following units into `/etc/systemd/system/` and adjust paths/state directory as required.

`arw-maintenance.service`

```ini
[Unit]
Description=ARW maintenance sweep
Wants=arw-server.service

[Service]
Type=oneshot
WorkingDirectory=/opt/arw
ExecStart=/usr/bin/systemctl stop arw-server.service
ExecStart=/opt/arw/scripts/maintenance.sh --state-dir /var/arw/state
ExecStart=/usr/bin/systemctl start arw-server.service
```

`arw-maintenance.timer`

```ini
[Unit]
Description=Weekly ARW maintenance sweep

[Timer]
OnCalendar=Sun *-*-* 03:00:00
Persistent=true

[Install]
WantedBy=timers.target
```

Enable the timer:

```bash
systemctl daemon-reload
systemctl enable --now arw-maintenance.timer
```

> Templates: `ops/systemd/arw-maintenance.service` and `ops/systemd/arw-maintenance.timer` mirror the snippets above; drop them into `/etc/systemd/system/`, tweak paths/consent, then enable the timer.

## Scheduling with Windows Task Scheduler

1. Copy `ops/windows/invoke-maintenance.ps1` to a stable path (for example `C:\ARW\ops\windows\invoke-maintenance.ps1`) and edit the configuration block to match your install paths, consent defaults, and service name. Use `-DryRun` for the first execution if you want a preview, `-Tasks @(...)` to run a subset, or `-SkipServiceStop` when you want to manage the service lifecycle yourself.
2. Import `ops/windows/arw-maintenance.xml` in Task Scheduler (`Action` → `Import Task`), then adjust:
   - **General** tab: the account that runs the task (must have rights to stop/start the service and touch the state directory) and select *Run with highest privileges*.
   - **Triggers**: update the schedule (default: Sunday 03:00).
   - **Actions**: point `Arguments` and `Working directory` at your edited script if you changed the defaults.
3. Test with **Run**. Review Task Scheduler history and the maintenance output to confirm pointer-migrate completed.

CLI alternative:

```powershell
schtasks /Create `
  /TN "ARW Maintenance" `
  /TR "pwsh.exe -NoProfile -File C:\ARW\ops\windows\invoke-maintenance.ps1" `
  /SC WEEKLY /D SUN /ST 03:00 `
  /RL HIGHEST
```

## Post-run verification

- Review the maintenance output (cron log, journal, or Task Scheduler history) for pointer-migrate results; dry runs print actions without modifications.
- Check Prometheus alerts (for example `ARWPlanGuardFailuresSpike`) after the sweep to ensure the planner is healthy.
- Commit any regenerated documentation (`scripts/maintenance.*` calls `stamp_docs_updated.py`) if you run the sweep inside a Git workspace.

See also: [Automation Ops Handbook](automation_ops.md) for broader operational guardrails and [Monitoring & Alerts](monitoring.md) for Grafana/Prometheus wiring.
