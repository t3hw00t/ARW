# ARW Server Alerting Quickstart

Updated: 2025-10-16
Type: Explanation

This guide shows how to wire the server's stability signals into monitoring and alerting.

## Prometheus

The server exposes Prometheus metrics at `/metrics`. Stability-related metrics:

- `arw_safe_mode_active` — 0/1 gauge indicating safe‑mode (recent crash) is active.
- `arw_safe_mode_until_ms` — epoch milliseconds until safe‑mode ends (0 if inactive).
- `arw_last_crash_ms` — epoch milliseconds of the last crash marker (0 if none).
- `arw_context_cascade_last_event_age_ms` - milliseconds since the newest episode processed by the context cascade (watch for large gaps).
- `arw_context_cascade_processed_last` / `arw_context_cascade_skipped_last` - number of episodes processed or skipped in the latest cascade sweep.
- `arw_context_cascade_last_event_id` - last event id seen by the cascade (monotonic).
- Prompt compression metrics exposed via `/v1/compress/prompt` instrumentation:
  - `arw_compression_prompt_requests_total`, `..._success_total`, `..._errors_total`
  - `arw_compression_prompt_primary_total`, `..._fallback_total`
  - `arw_compression_prompt_avg_latency_ms`, `..._avg_ratio`, `..._avg_pre_chars`, `..._avg_post_chars`, `..._avg_pre_bytes`, `..._avg_post_bytes`

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

  - alert: ARWContextCascadeStale
    expr: arw_context_cascade_last_event_age_ms > 900000
    for: 15m
    labels:
      severity: warning
    annotations:
      summary: "Context cascade has not processed episodes in > 15 minutes"
      description: |
        The cascade last processed an episode {{ $value | printf "%.0f" }} ms ago. Investigate the
        context.cascade task and recent episode volume.

  - alert: ARWPromptCompressionErrorRateHigh
    expr: rate(arw_compression_prompt_errors_total[5m])
          / clamp_min(rate(arw_compression_prompt_requests_total[5m]), 1e-6) > 0.2
    for: 10m
    labels:
      severity: warning
    annotations:
      summary: "Prompt compression error rate > 20% over 10 minutes"
      description: |
        Prompt compression backend is erroring frequently (ratio {{ printf "%.2f" $value }}). Inspect llmlingua subprocess logs
        and recent prompt payloads; disable compression via guardrails if needed.

  - alert: ARWPromptCompressionFallbackSpike
    expr: rate(arw_compression_prompt_fallback_total[5m])
          / clamp_min(rate(arw_compression_prompt_success_total[5m]), 1e-6) > 0.5
    for: 15m
    labels:
      severity: info
    annotations:
      summary: "Prompt compression falling back to noop > 50% of the time"
      description: |
        The primary compressor is unavailable (fallback ratio {{ printf "%.2f" $value }}). Check llmlingua availability and
        GPU/CPU load; consider disabling compression or scaling capacity.
```

## Dashboards (Grafana)

Panels to consider:
- Stat panel for `arw_safe_mode_active` (with threshold > 0 highlighting).
- Single stat for minutes since last crash: `(time() * 1000 - arw_last_crash_ms) / 60000`.
- Table for route latency: `arw_route_p95_ms` by `path`.
- Histogram panel for route latency percentiles via PromQL `histogram_quantile(0.95, sum by (path, le)(rate(arw_route_latency_seconds_bucket[5m])))` so you can alert on the true p95 alongside the rolling window value.
- Prompt compression cards: stacked stat for request/error counts, and time-series for `rate(arw_compression_prompt_errors_total[5m])` alongside `rate(arw_compression_prompt_fallback_total[5m])` to visualize backend health vs fallback frequency.

## Service Health Read-Model (JSON)

The server publishes `service.health` events and maintains a read-model at:

- `GET /state/service_health` (admin)
- `GET /state/service_status` (admin) — includes safe-mode, last crash, and last health

If you prefer JSON-driven alerting, use a JSON exporter (e.g., `prometheus-community/json_exporter`) to scrape `/state/service_status` and map fields to metrics for custom alert rules.
