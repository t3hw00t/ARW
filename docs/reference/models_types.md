---
title: Models Typed Shapes
---

# Models Typed Shapes

Updated: 2025-09-21
Type: Reference

Microsummary: Stable, typed response shapes for models endpoints used by UIs and clients. These align with the OpenAPI schemas exposed by the service.

- ModelsSummary: returned by `GET /admin/models/summary`
  - items: ModelItem[]
  - default: string
  - concurrency: ModelsConcurrency
  - metrics: ModelsMetrics

- ModelItem: elements in the models list (subset of manifest fields)
  - id: string
  - provider?: string
  - path?: string
  - sha256?: string
  - bytes?: number
  - status?: string
  - error_code?: string

- ModelsConcurrency: downloader concurrency snapshot
  - configured_max: number
  - available_permits: number
  - held_permits: number
  - hard_cap?: number
  - pending_shrink?: number (when a non-blocking shrink has remaining in-flight jobs)

- ModelsMetrics: counters + throughput estimate and nested detail
  - started, queued, admitted, resumed, canceled: number
  - completed: number (fresh downloads)
  - completed_cached: number (coalesced followers)
  - errors: number
  - bytes_total: number
  - ewma_mbps?: number (moving average download rate)
  - preflight_ok / preflight_denied / preflight_skipped: number
  - coalesced: number (followers served from cache)
  - inflight: ModelsInflightEntry[]
  - concurrency: ModelsConcurrency
  - jobs: ModelsJobSnapshot[]

- ModelsInflightEntry: active hash groups awaiting completion
  - sha256: string
  - primary: string (model id currently downloading)
  - followers: string[] (coalesced waiters)
  - count: number (total participants)

- ModelsJobs: active jobs snapshot (admin)
  - active: ActiveJob[]
  - inflight: ModelsInflightEntry[]
  - concurrency: ModelsConcurrency

- ActiveJob
  - model_id: string
  - job_id: string
  - corr_id?: string (download correlation id)

- ModelsHashes (paginated): returned by `GET /state/models_hashes`
  - total: number
  - count: number
  - limit: number
  - offset: number
  - items: HashItem[]

- HashItem
  - sha256: string
  - bytes: number
  - path: string
  - providers: string[]

Notes
- OpenAPI: `GET /spec/openapi.yaml` includes these schemas for codegen.
- SSE read‑models: live patches are published under `state.read.model.patch` with ids `models` and `models_metrics`.
- The raw models list (`GET /admin/models`) returns the persisted array with runtime fields and may include fields beyond `ModelItem`.
