---
title: Performance Presets
---

# Performance Presets
Updated: 2025-09-15
Type: How‑to

ARW ships with built‑in performance presets to adapt resource usage to your machine without hand‑tuning dozens of knobs.

Presets are applied at process start, seeding defaults for hot‑path tunables only if you haven’t set them explicitly. You can always override any variable.

## Quick Start
- Balanced (default, auto‑detected):
  - `ARW_PERF_PRESET=balanced arw-server`
- Power‑saver:
  - `ARW_PERF_PRESET=eco arw-server`
- High‑throughput dev box:
  - `ARW_PERF_PRESET=performance arw-server`
- Max out on big workstations:
  - `ARW_PERF_PRESET=turbo arw-server`

When unset, ARW auto‑detects a tier from CPU cores and total RAM and sets `ARW_PERF_PRESET_TIER` to the chosen tier.

Check effective tier in `/about` response or env:
```
curl -s http://127.0.0.1:8091/about | jq '.perf_preset? // {}'
echo $ARW_PERF_PRESET_TIER
```

## What Presets Tune
- HTTP Concurrency: `ARW_HTTP_MAX_CONC`
- Actions Queue Capacity: `ARW_ACTIONS_QUEUE_MAX`
- Context Scan Limit: `ARW_CONTEXT_SCAN_LIMIT`
- File Rehydrate Head Bytes: `ARW_REHYDRATE_FILE_HEAD_KB`
- Read‑model Cadences: `ARW_ROUTE_STATS_*`, `ARW_MODELS_METRICS_*`

The goal is stable latency and snappy UX:
- Keep admission bounded and fair, avoid overload stalls.
- Stream early and often; push heavy work off the request path.
- Prefer backpressure (429) over slow timeouts.

## Inspect & Override
Presets only provide defaults. To override, export the env var(s) you care about:
```
export ARW_PERF_PRESET=balanced
export ARW_HTTP_MAX_CONC=2048   # override one knob
arw-server
```

## Build vs Runtime
- Build profiles (`release`, `maxperf`) control compiler and binary characteristics.
- Presets control runtime behavior. They can be combined: e.g., `cargo build --profile maxperf` and run with `ARW_PERF_PRESET=performance`.

See also: Configuration (CONFIGURATION.md), Interactive Performance (guide/interactive_performance.md)

