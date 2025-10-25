# Ops Assets

Updated: 2025-10-24  
Type: Reference

This directory collects operator-facing bundles for Prometheus/Grafana monitoring and maintenance scheduling.

```
ops/
├─ out/                 # Latest exported Prometheus/Grafana assets (regenerated)
├─ systemd/             # Example service/timer units (arw-maintenance.*)
├─ cron/                # Example cron helper (arw-maintenance.sh)
├─ windows/             # Windows Task Scheduler helper + wrapper script
├─ access_matrix.yaml   # Role/permission reference
└─ …
```

## Exporting monitoring assets

From the repository root you can invoke either the shell helpers or the cross-platform Python scripts:

```bash
scripts/export_ops_assets.sh             # POSIX shell helper (writes to ops/out)
scripts/package_ops_assets.sh            # shell helper + tar.gz bundle
python scripts/export_ops_assets.py      # cross-platform helper
python scripts/package_ops_assets.py     # writes zip archive to dist/ops-assets.zip

python scripts/package_ops_assets.py \
  --out ops/out-prod \
  --archive dist/prod-ops-assets.zip     # customise paths
```

The generated directory/archive contains:

- `prometheus_recording_rules.yaml`
- `prometheus_alerting_rules.yaml`
- `grafana_quick_panels.json`

Copy the files to your monitoring host (for example `/etc/prometheus/rules/`), reload Prometheus/Alertmanager/Grafana, and import the Grafana JSON. Windows operators can transfer `dist/ops-assets.zip`. The deployment helper `python scripts/apply_ops_assets.py --rules-dir <dir> [...]` automates copying the rule files, triggering reloads, and importing the Grafana dashboard when supplied with credentials (see `docs/ops/monitoring.md` for examples).

## Scheduling maintenance

Templates for each platform:

- `ops/systemd/arw-maintenance.service` + `.timer` — install to `/etc/systemd/system/`, adjust paths/consent, then `systemctl enable --now arw-maintenance.timer`.
- `ops/cron/arw-maintenance.sh` — copy to `/etc/cron.weekly/`, tweak the environment block at the top, and `chmod +x`.
- `ops/windows/invoke-maintenance.ps1` + `ops/windows/arw-maintenance.xml` — edit the configuration block in the PowerShell script, then import the XML into Task Scheduler (or treat it as a reference for `schtasks`).

All templates call the maintenance helper (`scripts/maintenance.sh` or `scripts/maintenance.ps1`), which runs pointer canonicalisation after the usual clean/prune tasks.

See also:

- `docs/ops/maintenance.md` for detailed scheduling instructions
- `docs/ops/monitoring.md` for Prometheus/Grafana wiring and alert descriptions
