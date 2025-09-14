---
title: Cache Policy Manifest
---

# Cache Policy Manifest (Design + Mapping)

Updated: 2025-09-13

This document describes a small, declarative “cache policy” manifest you can use to express caching behaviors across ARW layers. It reflects research‑informed practices and maps to today’s implementation knobs where available. Some sections are forward‑looking; they are noted as planned.

## Goals

- Make caching behavior explicit, portable, and explainable.
- Prefer exactness and correctness (content addressing, validators) before approximation.
- Keep privacy: scope semantic caches per project/user by default; no cross‑project sharing.
- Favor incremental, high‑ROI wins (KV/prefix reuse; action cache; SSE deltas).

## Example (YAML)

```yaml
cache:
  action_cache:
    key: jcs(json(input, env, tool_version))
    store: cas://local-ssd
    ttl: 7d
    revalidate_on: [tool_version_change, secret_version_change]
  llm:
    kv_prefix_cache: on           # vLLM or llama.cpp prompt cache
    semantic_cache:
      embedder: "bge-small-en"
      sim_threshold: 0.92
      verifier: "mini-qa-judge"   # fast NLI or rule-based
      scope: per-user             # federate stats periodically
      negative_cache_ttl: 2h
  read_models:
    sse:
      format: json-patch          # RFC 6902
      resume: last-event-id       # SSE resume (planned)
      coalesce_ms: 250
      idle_publish_ms: 2000
  in_memory:
    policy: w-tinylfu             # or s3-fifo
    capacity_mb: 512
  disk:
    engine: rocksdb
    block_cache_uncompressed_mb: 512
    block_cache_compressed_mb: 512
    partitioned_index_filters: true
    secondary_cache: ssd
  edge:
    headers:
      cache_control: "public, max-age=60, stale-while-revalidate=300, stale-if-error=86400"
    request_coalescing: true
  compression:
    zstd_dictionary:
      per_type: [json_tool_output, patches]
      train_on: last_10k_samples
```

## Current Implementation Mapping

- Action Cache (tools):
  - Key: `sha256(tool_id@version + canonical JSON input)`.
  - Store: CAS under `{state_dir}/tools/by-digest/`.
  - In‑memory: Moka W‑TinyLFU front.
  - Env: `ARW_TOOLS_CACHE_TTL_SECS`, `ARW_TOOLS_CACHE_CAP`.
  - Admin: `GET /admin/tools/cache_stats`.
  - Events/metrics: `tool.cache` events, `/metrics` `arw_tools_cache_*`.

- LLM KV/prefix cache:
  - llama.cpp: client sends `cache_prompt: true`; server can run with `--prompt-cache <file>` for persistence.
  - vLLM: plan to rely on PagedAttention/prefix cache when adapter lands.

- Read‑models over SSE:
  - JSON Patch deltas (RFC 6902) with coalescing and idle publish via `state.read.model.patch`.
  - Models metrics (counters + EWMA): `GET /state/models_metrics`, SSE id=`models_metrics`.
  - Route stats (p95/ewma/hits/errors): `GET /state/route_stats`, SSE id=`route_stats`.
  - Env: `ARW_MODELS_METRICS_COALESCE_MS`, `ARW_MODELS_METRICS_PUBLISH_MS`, `ARW_ROUTE_STATS_COALESCE_MS`, `ARW_ROUTE_STATS_PUBLISH_MS`.

- Edge/HTTP validators:
  - Digest‑addressed blobs served with strong validators: `ETag:"<sha256>"`, `Last-Modified`, and `Cache-Control: public, max-age=31536000, immutable`.
  - Endpoint: `GET /admin/models/by-hash/:sha256` (egress‑gated; 304 on `If-None-Match`).

## Planned

- Semantic cache (per project/user) with verified reuse and SimHash/LSH prefilter.
- RocksDB tiers with partitioned filters + secondary (flash) cache.
- Zstd dictionaries trained per small JSON type.
- Peer/edge CAS for multi‑host dev (opt‑in).
- SSE resume via `Last-Event-ID` for read‑models.

## Notes

- This manifest is a design document today — it does not override env or code.
- Where possible, ARW maps policy concepts to env knobs and admin endpoints to keep changes incremental and transparent.
