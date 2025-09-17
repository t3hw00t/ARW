---
title: Performance Guardrails
---

# Performance Guardrails

Updated: 2025-09-17
Type: Explanation

This page expands on the [Roadmap → Performance Guardrails](../ROADMAP.md#performance-guardrails) summary. It documents the guardrails that keep ARW responsive on a single laptop yet ready for shared clusters without runaway resource use. Each section reuses the roadmap terminology so teams can jump between planning notes and implementation details.

See also: [Caching Layers](caching_layers.md) for low-level mechanics.

## Prompt reuse for inference

llama.cpp runs with `cache_prompt: true` clients and a persistent `--prompt-cache` so shared prefixes stay on disk between requests. Planned vLLM support will reuse prefix/KV blocks through PagedAttention. Both approaches bound GPU minutes and token churn by reusing already-paid-for context state instead of regenerating it.

## Action Cache (Bazel-style)

Deterministic tool executions are hashed by tool id/version, RFC‑8785 JSON input, and an environment signature. The Action Cache fronts a disk CAS with a W‑TinyLFU in-memory layer, TTL controls, and configurable capacity. Combined, these limits cap CPU re-execution, memory pressure, and disk growth to declared budgets while still delivering near-instant hits.

## Digest-addressed HTTP caching

Immutable artifacts such as model weights are served from `/admin/models/by-hash/:sha256` with `ETag:"<sha256>"`, `Last-Modified`, and long-lived `Cache-Control` headers. Clients can reuse responses safely, sharply reducing repeated downloads and keeping egress and bandwidth predictable.

## Request coalescing

A singleflight guard wraps identical cache misses and expensive reads. When many callers request the same work, only one execution proceeds and the rest share the result. This prevents stampedes that would otherwise exceed worker concurrency, CPU, or GPU allocations.

## Read-model SSE deltas

Live dashboards subscribe to RFC‑6902 JSON Patch deltas published over SSE. Bursts are coalesced (250 ms default) and long-lived clients resume via `Last-Event-ID`. The approach avoids replaying full snapshots, bounding both network throughput and the JSON patching work UIs must perform.

## Semantic and negative caches (planned)

Planned per-project semantic caches capture question→answer pairs plus negative results for retrieval. Hits are double-checked by a lightweight verifier before reuse. Privacy scopes determine who can read from a cache, while eviction policies and memory/disk caps prevent unbounded growth even as the system learns.

## Tiered storage & compression

Caches flow from RAM to RocksDB and an optional flash tier. Small JSON blobs use Zstd dictionaries to stay compact. These layers ensure hot data remains fast to access without ballooning resident memory, while colder entries persist efficiently until policy-based eviction.

## Instrumentation & policy manifests

Every guardrail emits counters: hit ratios, latency saved, suppressed duplicates, and semantic verifier outcomes surface in `/state/*` endpoints and Prometheus metrics. Operators feed those measurements into a declarative cache policy manifest that defines privacy scopes, fallbacks, and capacity ceilings so limits are enforced before user experience degrades.
