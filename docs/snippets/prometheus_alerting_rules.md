---
title: Prometheus Alerting Rules — ARW
---

# Prometheus Alerting Rules — ARW

Updated: 2025-10-25
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

      # Planner guard failures exceeding 5% of plans for 10 minutes
      - alert: ARWPlanGuardFailuresSpike
        expr: rate(arw_plan_guard_failures_total[10m])
              / clamp_min(rate(arw_plan_requests_total[10m]), 1e-6) > 0.05
        for: 10m
        labels:
          severity: warning
        annotations:
          summary: "Planner guard failures elevated (> 5% for 10m)"
          description: |
            Planner guard failures are {{ printf "%.2f" $value }} of total plans. Inspect compression policies,
            pointer consent metadata, or recent plan requests for malformed input.

      # Planner-driven autonomy throttles spike (>3 interrupts in 15m)
      - alert: ARWAutonomyPlanThrottleSpike
        expr: sum(increase(arw_autonomy_interrupts_total{reason=~"plan_guard_failures|plan_warnings"}[15m])) > 3
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Autonomy throttles triggered by planner (>3 in 15m)"
          description: |
            Planner feedback is repeatedly forcing autonomy into guided mode ({{ printf "%.0f" $value }}
            interrupts/15m). Review guard failures, the engagement ledger, and recent audit.log entries
            from /admin/autonomy/{lane}/engagement resets.

```

Tip: Pair alerts with routing labels/receivers (PagerDuty/Slack) in Alertmanager.
