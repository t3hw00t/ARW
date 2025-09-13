---
title: Caching Layers
---

# Caching Layers (Design + Implementation)

Updated: 2025-09-13

This document outlines a multi‑layer caching strategy for ARW, blending research‑informed ideas with practical, incremental implementations. It aims for high ROI first (latency, throughput, stability), while keeping privacy and determinism front‑and‑center.

## Layers

- LLM inference‑level (high ROI)
  - Prefix/KV reuse for repeated system/prompt prefixes. With llama.cpp, enable `cache_prompt: true` (client) and persistent `--prompt-cache` (server). With vLLM, rely on PagedAttention and automatic prefix caching.
  - Memory policy: schedule batches to share prefixes and keep KV blocks defragmented; prefer fixed‑size paging to reduce fragmentation.

- Tool/action cache (Bazel‑style)
  - Treat each tool invocation as a deterministic action keyed by a content hash of: tool id/version, canonical input (RFC‑8785 JSON), and an environment signature.
  - Store outputs in a content‑addressed store (CAS) and map `action_key → digest`. Replay on hit; execute on miss. Emit `Tool.Cache` events and publish lightweight counters.

- Semantic response cache (planned)
  - Cache Q→A pairs per project/user keyed by embeddings with a verifier gate. Only reuse when a thresholded match passes a quick check; otherwise seed the model with the cached answer for speculative decoding.
  - Context‑aware keys include turn‑level features and referenced tool outputs.

- Retrieval caches (planned)
  - Cache frequent ANN results and maintain a negative cache for “no useful doc” to save vector queries. Use LSH/SimHash buckets as a prefilter before expensive comparisons.

- Read‑models over SSE
  - Maintain small, incremental read‑models in process (e.g., route stats, models metrics) and publish RFC‑6902 JSON Patch deltas over SSE.
  - Coalesce bursts (250ms default) and publish an idle refresh (2s default). Resume with Last‑Event‑ID via the standard SSE API.

- In‑memory + on‑disk persistence
  - In‑memory: modern, robust eviction (W‑TinyLFU/S3‑FIFO). In ARW, we use Moka (W‑TinyLFU) for the Action Cache.
  - On‑disk: content‑addressed store under `{state_dir}`. Consider RocksDB with uncompressed+compressed block caches and a secondary flash tier for large, hot blobs.

- Edge & HTTP caching
  - Emit strong validators and immutable `Cache-Control` for digest‑addressed blobs. ARW serves `ETag:"<sha256>"`, `Last-Modified`, and `public, max-age=31536000, immutable` for `/admin/models/by-hash/:sha256`, and honors `If-None-Match`.
  - Stampede protection: coalesce identical misses with a singleflight mechanism.

## What’s implemented in ARW today

- llama.cpp client requests include `cache_prompt: true` enabling KV reuse.
- Tool Action Cache with Moka front + disk CAS back; RFC‑8785‑like canonicalization, singleflight, counters, Prometheus metrics, and admin stats.
- CAS blob serving with validators and 304 handling.
- Read‑models and deltas: models metrics and route stats publish RFC‑6902 patches with coalescing; UI panels consume them live.

## Metrics & measurement

- Report hit ratio (by layer), P95/P99 latency saved, bytes saved (post‑compression), stampede suppression rate, semantic false‑hit rate, and recompute budget.
- In ARW:
  - Tool Action Cache: `/admin/tools/cache_stats`, `Tool.Cache` events, and `/metrics` `arw_tools_cache_*`.
  - Models metrics: `/state/models_metrics` and `/metrics` `arw_models_download_*`.
  - Route stats: `/state/route_stats` and overlays in `/debug` (p95/ewma/hits/errors).

## Configuration knobs

- Action Cache: `ARW_TOOLS_CACHE_TTL_SECS`, `ARW_TOOLS_CACHE_CAP`.
- Route stats: `ARW_ROUTE_STATS_COALESCE_MS`, `ARW_ROUTE_STATS_PUBLISH_MS`.
- Models metrics: `ARW_MODELS_METRICS_COALESCE_MS`, `ARW_MODELS_METRICS_PUBLISH_MS`.
- CAS blob serving: no special knobs; follows the digest semantics automatically.

## Next steps (tracked in Backlog)

- Verified semantic cache (per‑project, per‑user; privacy‑preserving learning of thresholds).
- Context‑aware keys and SimHash prefilter for semantic caches.
- RocksDB tier for persistent hot sets (tools/semantic/embeddings) with Zstd dictionaries for small JSON types.
- Peer/edge CAS for artifacts (opt‑in; IPLD/libp2p style gossip for multi‑host dev).

