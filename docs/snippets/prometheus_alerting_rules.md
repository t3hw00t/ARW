---
title: Prometheus Alerting Rules — ARW
---

# Prometheus Alerting Rules — ARW

Updated: 2025-09-22
Type: How‑to

Example alerting rules for common resource conditions. Tune thresholds and durations to your environment. GPU alerts depend on the GPU telemetry pack; if the `arw_gpu_*` metrics are absent, drop or postpone those rules.

## Alerts.yaml

```yaml
groups:
  - name: arw-alerts
    interval: 30s
    rules:
      # CPU high for 5 minutes (use recording rule if present)
      - alert: ARWCPUHigh
        expr: (arw:cpu_percent_avg:5m OR arw_cpu_percent_avg) > 90
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "ARW CPU usage high (> 90% for 5m)"
          description: |
            CPU avg is {{ $value | printf "%.1f" }}%% for 5 minutes.

      # Memory usage high for 10 minutes
      - alert: ARWMemoryHigh
        expr: (arw:mem_usage_percent OR (100 * arw_mem_bytes_used / arw_mem_bytes_total)) > 90
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "ARW memory usage high (> 90% for 10m)"
          description: |
            Memory usage is {{ $value | printf "%.1f" }}%% for 10 minutes.

      # GPU memory usage high for 5 minutes
      - alert: ARWGpuMemoryHigh
        expr: (arw:gpu_mem_usage_percent OR (100 * sum(arw_gpu_adapter_memory_bytes{kind="used"}) / sum(arw_gpu_adapter_memory_bytes{kind="total"}))) > 95
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "ARW GPU memory usage high (> 95% for 5m)"
          description: |
            GPU memory usage is {{ $value | printf "%.1f" }}%% for 5 minutes.

      # Cascade worker stale (no episodes processed for 15 minutes)
      - alert: ARWContextCascadeStale
        expr: arw_context_cascade_last_event_age_ms > 900000
        for: 15m
        labels:
          severity: warning
        annotations:
          summary: "Context cascade stale (> 15m without processing episodes)"
          description: |
            Cascade last processed an episode {{ $value | printf "%.0f" }} ms ago. Inspect the
            context.cascade task, recent episode volume, and logs on the ARW server.

```

Tip: Pair alerts with routing labels/receivers (PagerDuty/Slack) in Alertmanager.
