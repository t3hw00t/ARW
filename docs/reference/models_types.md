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
  - pending_shrink?: number

- ModelsMetrics: counters + throughput estimate
  - started: number
  - queued: number
  - admitted: number
  - resumed: number
  - canceled: number
  - completed: number
  - completed_cached: number
  - errors: number
  - bytes_total: number
  - ewma_mbps?: number

- ModelsJobs: active jobs snapshot (admin)
  - active: ActiveJob[]
  - inflight_hashes: string[]
  - concurrency: ModelsConcurrency (includes pending_shrink when non‑blocking shrink left remainder)

- ActiveJob
  - model_id: string
  - job_id: string

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
