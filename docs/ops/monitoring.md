---
title: Monitoring & Alerts
---

# Monitoring & Alerts

Updated: 2025-10-09
Type: How‑to

This note captures the minimal wiring needed to surface ARW metrics in Prometheus/Grafana and keep an eye on the legacy shut-down counter.

## Prometheus

1. Drop the recording/alerting rules from the snippets directory into your Prometheus configuration:
   ```yaml
   # prometheus.yml
   rule_files:
     - /etc/prometheus/rules/arw-recording-rules.yaml
     - /etc/prometheus/rules/arw-alerting-rules.yaml
   ```

2. Copy the YAML code blocks from the snippets into your rule files (the fenced ` ```yaml ... ``` ` sections in each snippet). You can let `scripts/export_ops_assets.sh` collect the latest assets into a staging directory:
   ```bash
   just ops-export /etc/prometheus/rules
   # or run the script directly
   ./scripts/export_ops_assets.sh --out /etc/prometheus/rules
   # or set ARW_EXPORT_OUTDIR for alternative locations
   ```
   If you prefer a manual extraction, the following helper works too:
   ```bash
   install -d /etc/prometheus/rules
   awk '/```yaml/{flag=1;next}/```/{flag=0}flag' docs/snippets/prometheus_recording_rules.md > /etc/prometheus/rules/arw-recording-rules.yaml
   awk '/```yaml/{flag=1;next}/```/{flag=0}flag' docs/snippets/prometheus_alerting_rules.md > /etc/prometheus/rules/arw-alerting-rules.yaml
   ```

   The alert rules include `ARWLegacyCapsuleHeadersSeen` and `ARWContextCascadeStale` so you get early warning if legacy callers resurface or if the cascade worker stops processing fresh episodes.

3. Reload Prometheus:
   ```bash
   curl -X POST http://127.0.0.1:9090/-/reload
   ```

## Alertmanager

If you use Alertmanager, add routes/receivers for the legacy alerts (example):

```yaml
# alertmanager.yml
route:
  receiver: default
  routes:
    - matchers:
        - alertname = "ARWLegacyCapsuleHeadersSeen"
      receiver: legacy-warning

receivers:
  - name: default
    webhook_configs:
      - url: https://ops.example.com/webhook/default
  - name: legacy-warning
    slack_configs:
      - channel: "#arw-migrations"
        send_resolved: true
```

Then reload Alertmanager (`/-/reload`).

## Grafana panels

Import the “Quick Panels” snippet into a dashboard so the legacy counters are visible at a glance (the export script above drops `grafana_quick_panels.json` alongside the Prometheus rules):

1. Grafana → Dashboards → Import → paste the JSON from `docs/snippets/grafana_quick_panels.md`.
2. Select your Prometheus datasource when prompted (`DS_PROMETHEUS`).
3. Pin the stat panel (“Legacy Capsule Headers (15m)”) to the migration dashboard.

### Snappy Latency Budget Panel

Snappy publishes a read-model (`id="snappy"`) that surfaces the worst protected routes, their p95 latency, and whether the full-result budget is breached. Mirror the same signal in Grafana with a table panel:

- Query: `topk(5, max_over_time(arw_route_p95_ms{route=~"/admin/debug|/state/.*"}[5m]))`
- Add calculated fields for the configured budget (see `ARW_SNAPPY_FULL_RESULT_P95_MS`) so the table highlights overruns.
- Add a stat panel keyed to `arw_snappy_breach_total` (exposed via `/metrics`) for an at-a-glance breach indicator.
- Layer in the histogram export for real percentile math: `histogram_quantile(0.95, sum by (path, le)(rate(arw_route_latency_seconds_bucket{path=~"/admin/debug|/state/.*"}[5m])))`. Keep the result next to `arw_route_p95_ms` so ops can compare the streaming p95 with the Prometheus-calculated percentile.
- Cross-link the panel description to the Hub’s Metrics sidecar (“Snappy detail”) so operators can jump from dashboards to the live SSE feed when triaging spikes.

## Staging checklist

Before cutting any legacy traffic, verify in staging:

- **Start scripts**: run `scripts/start.sh` (or `scripts/start.ps1 -ServiceOnly`) and confirm `/state/egress/settings` reports `proxy_enable=true` and `dns_guard_enable=true`. If automation requires these disabled, document the override before pushing to production.
- **Metrics**: confirm `arw_legacy_capsule_headers_total` stays at zero for at least 24 hours.
- **Alerts**: ensure the new Prometheus rules fire in staging by temporarily issuing a request with the deprecated gate header (for example `curl -H 'X-ARW-Capsule: {}' ...` against a compatibility shim), then acknowledge and clean up.
- **Smoke**: run `scripts/check_legacy_surface.sh` (or hit `/debug` manually) to confirm the legacy alias stays 404; when a server is online the helper also exercises `/admin/debug` and legacy capsule headers.
- **Evidence**: export `ARW_LEGACY_CHECK_REPORT=/var/log/arw/legacy-surface-$(date +%Y%m%dT%H%M%S).txt` so the smoke script writes a report you can attach to the change request or staging journal.

These checks keep the legacy-retirement tasks measurable and ensure the defaults you rely on in production match what operators see locally.
