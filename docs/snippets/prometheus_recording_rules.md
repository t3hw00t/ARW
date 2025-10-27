---
title: Prometheus Recording Rules — ARW
---

# Prometheus Recording Rules — ARW

Updated: 2025-10-27
Type: How‑to

Recording rules precompute common expressions so dashboards and alerts can use short, stable series names. GPU-related rules rely on the upcoming GPU telemetry pack; keep or remove them based on whether the `arw_gpu_*` metrics are present in your deployment.

## Example rules.yaml

```yaml
groups:
  - name: arw
    interval: 30s
    rules:
      # Smooth CPU percent over 5 minutes
      - record: arw:cpu_percent_avg:5m
        expr: avg_over_time(arw_cpu_percent_avg[5m])

      # Memory usage percent (instant)
      - record: arw:mem_usage_percent
        expr: 100 * arw_mem_bytes_used / arw_mem_bytes_total

      # Swap usage percent (instant)
      - record: arw:swap_usage_percent
        expr: 100 * arw_swap_bytes_used / arw_swap_bytes_total

      # GPU memory usage percent (instant, across adapters)
      - record: arw:gpu_mem_usage_percent
        expr: |
          100 * sum(arw_gpu_adapter_memory_bytes{kind="used"})
                 / sum(arw_gpu_adapter_memory_bytes{kind="total"})

      # VRAM total by vendor (GiB)
      - record: arw:gpu_vram_total_gib:vendor
        expr: |
          sum by (vendor) (
            arw_gpu_adapter_memory_bytes{kind="total"}
            * on (index) group_left(vendor) arw_gpu_adapter_info
          ) / (1024*1024*1024)

      # SSE sent events per minute (smoothed over 5 minutes)
      - record: arw:sse_sent_per_min:5m
        expr: 60 * rate(arw_events_sse_sent_total[5m])

      # SSE errors ratio over 5 minutes (errors / sent)
      - record: arw:sse_errors_ratio:5m
        expr: rate(arw_events_sse_errors_total[5m]) / clamp_min(rate(arw_events_sse_sent_total[5m]), 1e-6)

      # SSE de-duplication miss ratio over 5 minutes (misses / (hits + misses))
      - record: arw:sse_dedup_miss_ratio:5m
        expr: |
          rate(arw_events_sse_dedup_misses_total[5m])
          / clamp_min(rate(arw_events_sse_dedup_hits_total[5m]) + rate(arw_events_sse_dedup_misses_total[5m]), 1e-6)
```

Usage:
- Reference `arw:cpu_percent_avg:5m`, `arw:mem_usage_percent`, and `arw:gpu_mem_usage_percent` directly in panels and alerts.
- Vendor breakdown via `arw:gpu_vram_total_gib:vendor`.
