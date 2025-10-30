---
title: Performance Presets
---

# Performance Presets
Updated: 2025-10-26
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

### Eco Preset Details
- HTTP concurrency: `ARW_HTTP_MAX_CONC=128` by default on eco.
- Worker/queue caps: `ARW_WORKERS_MAX=4`, `ARW_ACTIONS_QUEUE_MAX=64` to keep throughput predictable.
- Tool cache: `ARW_TOOLS_CACHE_TTL_SECS=300`, `ARW_TOOLS_CACHE_CAP=256` to avoid reclaim churn on low‑memory hosts.
- Low‑power hints: `ARW_PREFER_LOW_POWER=1`, `ARW_LOW_POWER=1`, and OCR low‑power hints enabled (`ARW_OCR_PREFER_LOW_POWER=1`, `ARW_OCR_LOW_POWER=1`).
- Quieter logs by default: access log and UA/referrer flags off (`ARW_ACCESS_LOG=0`, `ARW_ACCESS_UA=0`, `ARW_ACCESS_UA_HASH=0`, `ARW_ACCESS_REF=0`).
- SSE payload decoration off: `ARW_EVENTS_SSE_DECORATE=0`.
- Runtime watcher cooldown: `ARW_RUNTIME_WATCHER_COOLDOWN_MS=1500` to reduce churn.
- Memory embed backfill: smaller batches and longer idle (`ARW_MEMORY_EMBED_BACKFILL_BATCH=64`, `ARW_MEMORY_EMBED_BACKFILL_IDLE_SEC=600`).
- OpenTelemetry exporters remain opt‑in (no preset change) — set `ARW_OTEL=1`/`ARW_OTEL_METRICS=1` explicitly if desired.
- Sanity-check the Rust toolchain on low-spec hosts with [Quick Smoke](quick_smoke.md) (`just quick-smoke` / `mise run quick:smoke`) before installing the full docs/UI toolchain.
- Honors `ARW_PERSONA_VIBE_HISTORY_RETAIN` so persona telemetry history stays lean on eco hosts.
- Guardrails such as `scripts/triad_smoke.sh` can default to the eco preset; override with `TRIAD_SMOKE_PERF_PRESET` or `ARW_PERF_PRESET` when you need other tiers during smoke runs.
- Override any value manually when a workload needs more headroom; explicit env vars always win over preset defaults.

## What Presets Tune
- HTTP Concurrency: `ARW_HTTP_MAX_CONC`
- Actions Queue Capacity: `ARW_ACTIONS_QUEUE_MAX`
- Action Worker Pool: `ARW_WORKERS` (auto-detected x2 cores, capped at 32 unless overridden by `ARW_WORKERS_MAX`)
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

### Check the Effective Tier

- Programmatically via `/about`:
```
curl -s http://127.0.0.1:8091/about | jq '.perf_preset'
```
- Quick diagnostic helper (repo root):
  - Just: `just preset-diag`
  - Mise: `mise run preset:diag`
  Prints the current tier and a few key knobs reported by `/about`, and lists any local env overrides.

## Build vs Runtime
- Build profiles (`release`, `maxperf`) control compiler and binary characteristics.
- Presets control runtime behavior. They can be combined: e.g., `cargo build --profile maxperf` and run with `ARW_PERF_PRESET=performance`.

See also: Configuration (CONFIGURATION.md), Interactive Performance (guide/interactive_performance.md)
