---
title: Metrics & Insights
---

# Metrics & Insights
{ .topic-trio style="--exp:.6; --complex:.7; --complicated:.6" data-exp=".6" data-complex=".7" data-complicated=".6" }

Updated: 2025-09-17
Type: How‑to

## Overview
- ARW collects lightweight, privacy‑respecting metrics locally to help you tune and understand behavior.
- Route metrics: hits, errors, EWMA latency, p95 latency, last/max latency, last status.
- Event counters: totals by event kind from the in‑process event bus.
- Metacognition: calibration (Brier/ECE), risk–coverage (selective prediction), competence by domain/tool, resource forecast accuracy (tokens/latency/$ MAE), safety outcomes, and self‑model stability.

## Endpoints
- GET `/introspect/stats` → `{ events, routes }` where `routes.by_path["/path"]` has `hits`, `errors`, `ewma_ms`, `p95_ms`, `last_ms`, `max_ms`, `last_status`.

## UI
- Open `/debug` and toggle “Insights”.
- See Event totals and the top 3 routes by p95 latency (also shows EWMA and error counts).
- Copy the JSON snapshot via “Copy stats”.

## Security
- `/introspect/*` surfaces are gated by default; see Developer Security Notes.

## Prometheus Exposition

- Endpoint: `GET /metrics` (text/plain; Prometheus exposition format)
- Selected counters and gauges:
  - `arw_bus_*` — event bus totals and receiver counts
  - `arw_http_route_*` — per-route hits/errors and latency histogram (p95 available via UI)
  - `arw_models_download_*` — models download lifecycle counters and EWMA throughput
  - `arw_tools_cache_*` — action cache hits/miss/coalesced and capacity/TTL
  - `arw_build_info{service,version,sha}` — build metadata
- Trust (RPU):
    - `arw_rpu_trust_last_reload_ms` — epoch ms of last trust store reload
    - `arw_rpu_trust_issuers` — current trust issuers count

### GPU/NPU metrics examples (PromQL)

- Adapters count (GPU/NPU):
```
arw_gpu_adapters
arw_npu_adapters
```

- Total GPU memory across adapters (bytes) and usage percent:
```
sum(arw_gpu_adapter_memory_bytes{kind="total"})
100 * sum(arw_gpu_adapter_memory_bytes{kind="used"}) / sum(arw_gpu_adapter_memory_bytes{kind="total"})
```

- GPU memory total by vendor (joins `vendor` from `arw_gpu_adapter_info`):
```
sum by (vendor) (
  arw_gpu_adapter_memory_bytes{kind="total"}
  * on (index) group_left(vendor) arw_gpu_adapter_info
)
```

- Top 5 busiest adapters (percent):
```
topk(5, arw_gpu_adapter_busy_percent)
```

- Max busy percent by vendor (join vendor labels via adapter info):
```
max by (vendor) (
  arw_gpu_adapter_busy_percent
  * on (index) group_left(vendor) arw_gpu_adapter_info
)
```

Notes
- `arw_gpu_adapter_info` is a 1‑valued series used to carry labels (`index`, `vendor_id`, `vendor`, `name`). Use a label join (`on(index) group_left(...)`) to attach those labels to other adapter metrics.
- Memory `kind` label is one of `total` or `used`.

See also:
- Snippets → Prometheus Recording Rules — ARW
- Snippets → Grafana — Quick Panels (CPU/Mem/GPU)
- Snippets → Prometheus Alerting Rules — ARW

Example scrape minimal config (Prometheus):
```
scrape_configs:
  - job_name: 'arw'
    static_configs:
      - targets: ['127.0.0.1:8091']
        labels:
          instance: 'local'
    metrics_path: /metrics
```

## Tuning Tips
- Use p95 to find outliers; EWMA helps watch short‑term drift.
- Send a “latency” signal in the Self‑Learning panel targeting a hot route; Analyze; consider applying the suggested `http_timeout_secs`.
- Consider switching to the “balanced” profile during high error periods.
- For reliability: enable the Metacognition unit, set a target coverage (e.g., 90%), and monitor risk–coverage and calibration plots before widening scope.

## Observability Discipline
- Four golden signals: latency, traffic, errors, saturation — at tool/model/runtime granularity.
- Per‑episode timelines: obs → belief → intent → action; include streamed tokens and tool I/O.
- Per‑project aggregates: success rates, retrieval diversity, cost, and error classes over time.
- Exportable traces: correlation id and spans attach to problem details and event envelopes.
