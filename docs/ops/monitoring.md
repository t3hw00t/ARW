---
title: Monitoring & Alerts
---

# Monitoring & Alerts

Updated: 2025-09-22
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

2. Copy the YAML code blocks from the snippets into your rule files (the fenced ` ```yaml ... ``` ` sections in each snippet). A quick helper:
   ```bash
   install -d /etc/prometheus/rules
   awk '/```yaml/{flag=1;next}/```/{flag=0}flag' docs/snippets/prometheus_recording_rules.md > /etc/prometheus/rules/arw-recording-rules.yaml
   awk '/```yaml/{flag=1;next}/```/{flag=0}flag' docs/snippets/prometheus_alerting_rules.md > /etc/prometheus/rules/arw-alerting-rules.yaml
   ```

   The alert rules include `ARWLegacyCapsuleHeadersSeen` so you will get early warning if any clients still hit compatibility headers while you plan the legacy turn-down.

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

Import the “Quick Panels” snippet into a dashboard so the legacy counters are visible at a glance:

1. Grafana → Dashboards → Import → paste the JSON from `docs/snippets/grafana_quick_panels.md`.
2. Select your Prometheus datasource when prompted (`DS_PROMETHEUS`).
3. Pin the stat panel (“Legacy Capsule Headers (15m)”) to the migration dashboard.

## Staging checklist

Before cutting any legacy traffic, verify in staging:

- **Start scripts**: run `scripts/start.sh` (or `scripts/start.ps1`) and confirm `/state/egress/settings` reports `proxy_enable=true` and `dns_guard_enable=true`. If automation requires these disabled, document the override before pushing to production.
- **Metrics**: confirm `arw_legacy_capsule_headers_total` stays at zero for at least 24 hours.
- **Alerts**: ensure the new Prometheus rules fire in staging by temporarily issuing a legacy request (`curl -H 'X-ARW-Gate: {}' ...`), then acknowledge and clean up.
- **Smoke**: hitting `/debug` should return 404; update client configs or bookmarks if it does not.

These checks keep the legacy-retirement tasks measurable and ensure the defaults you rely on in production match what operators see locally.
