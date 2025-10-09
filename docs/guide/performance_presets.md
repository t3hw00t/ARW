---
title: Performance Presets
---

# Performance Presets
Updated: 2025-10-09
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
- Context Working Set: `ARW_CONTEXT_K`, `ARW_CONTEXT_EXPAND_PER_SEED`, `ARW_CONTEXT_DIVERSITY_LAMBDA`, `ARW_CONTEXT_MIN_SCORE`, `ARW_CONTEXT_LANES_DEFAULT`, `ARW_CONTEXT_LANE_BONUS`, `ARW_CONTEXT_EXPAND_QUERY`, `ARW_CONTEXT_EXPAND_QUERY_TOP_K`, `ARW_CONTEXT_SCORER`, `ARW_CONTEXT_STREAM_DEFAULT`, `ARW_CONTEXT_COVERAGE_MAX_ITERS`
- File Rehydrate Head Bytes: `ARW_REHYDRATE_FILE_HEAD_KB`
- Read‑model Cadences: `ARW_ROUTE_STATS_*`, `ARW_MODELS_METRICS_*`

The goal is stable latency and snappy UX:
- Keep admission bounded and fair, avoid overload stalls.
- Stream early and often; push heavy work off the request path.
- Prefer backpressure (429) over slow timeouts.

## Context Streaming & Coverage

`/context/assemble` now supports server-sent events (SSE) so clients can render seeds, expansions, and selections as soon as they are discovered. `ARW_CONTEXT_STREAM_DEFAULT` controls whether requests opt-in to streaming when callers omit the `stream` flag. Each preset also seeds `ARW_CONTEXT_COVERAGE_MAX_ITERS`, giving the corrective coverage loop (CRAG) room to widen lanes or relax thresholds automatically when the initial working set looks sparse.

Query expansion is governed by `ARW_CONTEXT_EXPAND_QUERY` and `ARW_CONTEXT_EXPAND_QUERY_TOP_K`. When enabled, the working-set builder synthesizes a pseudo-relevance embedding from the top seeds, re-queries the hybrid index, and merges those candidates (tagged `expanded_query`) before selection. Combined with pluggable scorers (`ARW_CONTEXT_SCORER`), this keeps retrieval adaptive without hand-tuning for each workload tier.

### Metrics

The working-set pipeline exposes `metrics` counters and histograms so you can verify preset choices:

- `arw_context_phase_duration_ms{phase="retrieve|query_expand|link_expand|select|total"}` — phase timings.
- `arw_context_seed_candidates_total`, `arw_context_link_expansion_total`, `arw_context_query_expansion_total`, `arw_context_selected_total` — per-lane cardinality of seeds, expansions, and final picks.
- `arw_context_scorer_used_total{scorer=...}` — scorer usage per request.

Scrape the unified `/metrics` endpoint (or whichever exporter you wire up) to watch how presets behave under load and to decide when to bump budgets.

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
