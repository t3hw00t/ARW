---
title: Prometheus Alerting Rules — ARW
---

# Prometheus Alerting Rules — ARW

Updated: 2025-10-24
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

      # Prompt compression backend failing (errors / requests > 20% for 10m)
      - alert: ARWPromptCompressionErrorRateHigh
        expr: rate(arw_compression_prompt_errors_total[5m])
              / clamp_min(rate(arw_compression_prompt_requests_total[5m]), 1e-6) > 0.20
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Prompt compression error rate high (> 20% for 10m)"
          description: |
            Prompt compression backend error ratio is {{ printf "%.2f" $value }}. Inspect llmlingua
            subprocess logs and recent prompt payloads; disable compression if quality is impacted.

      # Prompt compression fallback ratio high (primary unavailable for >50% of successes)
      - alert: ARWPromptCompressionFallbackSpike
        expr: rate(arw_compression_prompt_fallback_total[5m])
              / clamp_min(rate(arw_compression_prompt_success_total[5m]), 1e-6) > 0.5
        for: 15m
        labels:
          severity: info
        annotations:
          summary: "Prompt compression fallback ratio elevated (> 50% for 15m)"
          description: |
            Primary compressor is frequently unavailable. Check llmlingua availability and system load;
            consider scaling capacity or temporarily disabling compression.

```

Tip: Pair alerts with routing labels/receivers (PagerDuty/Slack) in Alertmanager.
