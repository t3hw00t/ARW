# ARW Server Alerting Quickstart

Updated: 2025-09-27

This guide shows how to wire the server's stability signals into monitoring and alerting.

## Prometheus

The server exposes Prometheus metrics at `/metrics`. Stability-related metrics:

- `arw_safe_mode_active` — 0/1 gauge indicating safe‑mode (recent crash) is active.
- `arw_safe_mode_until_ms` — epoch milliseconds until safe‑mode ends (0 if inactive).
- `arw_last_crash_ms` — epoch milliseconds of the last crash marker (0 if none).

Example Prometheus alerting rules (YAML):

```
groups:
- name: arw-stability
  rules:
  - alert: ARWSafeModeActive
    expr: arw_safe_mode_active > 0
    for: 5m
    labels:
      severity: warning
    annotations:
      summary: "ARW safe-mode active"
      description: "ARW is in safe-mode due to recent crashes. Investigate last crash and restart cadence."

  - alert: ARWLastCrashRecent
    expr: (time() * 1000 - arw_last_crash_ms) < 600000
    for: 0m
    labels:
      severity: info
    annotations:
      summary: "Recent ARW crash detected"
      description: "ARW observed a recent crash within the last 10 minutes."
```

## Dashboards (Grafana)

Panels to consider:
- Stat panel for `arw_safe_mode_active` (with threshold > 0 highlighting).
- Single stat for minutes since last crash: `(time() * 1000 - arw_last_crash_ms) / 60000`.
- Table for route latency: `arw_route_p95_ms` by `path`.
- Histogram panel for route latency percentiles via PromQL `histogram_quantile(0.95, sum by (path, le)(rate(arw_route_latency_seconds_bucket[5m])))` so you can alert on the true p95 alongside the rolling window value.

## Service Health Read-Model (JSON)

The server publishes `service.health` events and maintains a read-model at:

- `GET /state/service_health` (admin)
- `GET /state/service_status` (admin) — includes safe-mode, last crash, and last health

If you prefer JSON-driven alerting, use a JSON exporter (e.g., `prometheus-community/json_exporter`) to scrape `/state/service_status` and map fields to metrics for custom alert rules.
