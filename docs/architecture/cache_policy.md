---
title: Cache Policy Manifest
---

# Cache Policy Manifest

Updated: 2025-10-09
Type: Explanation

This document describes a small, declarative “cache policy” manifest you can use to express caching behaviors across ARW layers. It reflects research‑informed practices and maps to today’s implementation knobs where available. Sections marked as planned remain aspirational; the rest ship today.

## Goals

- Make caching behavior explicit, portable, and explainable.
- Prefer exactness and correctness (content addressing, validators) before approximation.
- Keep privacy: scope semantic caches per project/user by default; no cross‑project sharing.
- Favor incremental, high‑ROI wins (KV/prefix reuse; action cache; SSE deltas).

## Example (YAML)

```yaml
cache:
  action_cache:
    ttl: 7d
    ttl_secs: 604800 # optional explicit seconds; overrides `ttl` if both are set
    capacity: 4096
    allow: [demo.echo, http.fetch]
    deny: [fs.patch]
  read_models:
    sse:
      coalesce_ms: 250
      idle_publish_ms: 2000

  # Planned fields (documented below) stay commented until their implementations land:
  # llm:
  #   kv_prefix_cache: on
  #   semantic_cache:
  #     embedder: "bge-small-en"
  #     sim_threshold: 0.92
  #     verifier: "mini-qa-judge"
  #     scope: per-user
  #     negative_cache_ttl: 2h
  # edge:
  #   headers:
  #     cache_control: "public, max-age=60, stale-while-revalidate=300, stale-if-error=86400"
  #   request_coalescing: true
  # compression:
  #   zstd_dictionary:
  #     per_type: [json_tool_output, patches]
  #     train_on: last_10k_samples
```

Place the manifest at `configs/cache_policy.yaml` (or set `ARW_CACHE_POLICY_FILE=/path/to/manifest.yaml`). `arw-server` loads it on startup and applies the supported keys to the environment before other services spin up.

```bash
# Example: try the sample policy
cp configs/cache_policy.example.yaml configs/cache_policy.yaml
ARW_DEBUG=1 cargo run -p arw-server
```

Logs identify changed keys, existing overrides, matches that already satisfied the desired value, and any parsing warnings:

```
INFO cache policy manifest applied applied="ARW_TOOLS_CACHE_TTL_SECS=604800,ARW_TOOLS_CACHE_CAP=4096"
INFO environment overrides take precedence overrides=["ARW_TOOLS_CACHE_ALLOW"]
INFO cache policy manifest retained retained=["ARW_TOOLS_CACHE_CAP"]
WARN cache policy manifest warning warning="failed to parse cache.action_cache.ttl value: String(\"later\")"
```

Assignments also show up in tests (`crates/arw-core/src/cache_policy.rs`) so manifest fields stay type-checked.

## Loader (Today)

Supported fields map directly to environment variables:

| Manifest key | Env var | Notes |
| --- | --- | --- |
| `cache.action_cache.ttl` / `ttl_secs` | `ARW_TOOLS_CACHE_TTL_SECS` | Accepts numbers or duration strings (`7d`, `15m`, `2500ms`). `ttl_secs` wins when both keys are present. |
| `cache.action_cache.capacity` / `cap` | `ARW_TOOLS_CACHE_CAP` | Sets action-cache entry capacity. |
| `cache.action_cache.allow` | `ARW_TOOLS_CACHE_ALLOW` | Deduplicated CSV of tool ids allowed to cache. |
| `cache.action_cache.deny` | `ARW_TOOLS_CACHE_DENY` | CSV of tools forced to bypass cache. |
| `cache.read_models.sse.coalesce_ms` | `ARW_ROUTE_STATS_COALESCE_MS`, `ARW_MODELS_METRICS_COALESCE_MS` | Keeps read-model flood control in sync. |
| `cache.read_models.sse.idle_publish_ms` | `ARW_ROUTE_STATS_PUBLISH_MS`, `ARW_MODELS_METRICS_PUBLISH_MS` | Controls idle publish cadence. |

When a variable is already set in the environment or process supervisor, the loader records the override and leaves the existing value in place. Matching values are tagged as `already_set_same_value`.

## Current Implementation Mapping

- Action Cache (tools):
  - Key: `sha256(tool_id@version + canonical JSON input)`.
  - Store: CAS under `{state_dir}/tools/by-digest/`.
  - In-memory: Moka W-TinyLFU front.
  - Env: `ARW_TOOLS_CACHE_TTL_SECS`, `ARW_TOOLS_CACHE_CAP`, `ARW_TOOLS_CACHE_MAX_PAYLOAD_BYTES`, `ARW_TOOLS_CACHE_ALLOW`, `ARW_TOOLS_CACHE_DENY` (configure via the manifest). Payload limits accept suffixes (`kb`, `mb`, `gb`) or `off/0` to disable.
  - Admin: `GET /admin/tools/cache_stats` (fields include hit, miss, coalesced waiters, bypass, payload_too_large, capacity, TTL, per-entry limit, entries).
  - Events/metrics: `tool.cache` events (outcomes include `hit`, `miss`, `coalesced`, `not_cacheable`, `error`; reasons now include `payload_too_large` when the per-entry cap skips storage) and `/metrics` counters such as `arw_tools_cache_hits`, `arw_tools_cache_miss`, `arw_tools_cache_coalesced`, `arw_tools_cache_coalesced_waiters`, `arw_tools_cache_error`, `arw_tools_cache_bypass`. The admin snapshot also reports `payload_too_large` totals so operators can see how often oversized responses were skipped.
  - Stampede control: identical in-flight tool calls coalesce behind a singleflight guard; followers block until the leader stores or fails, then reuse the cached result.

- LLM KV/prefix cache:
  - llama.cpp: client sends `cache_prompt: true`; server can run with `--prompt-cache <file>` for persistence.
  - vLLM: plan to rely on PagedAttention/prefix cache when adapter lands.

- Read‑models over SSE:
  - JSON Patch deltas (RFC 6902) with coalescing and idle publish via `state.read.model.patch`.
  - Models metrics (counters + EWMA): `GET /state/models_metrics`, SSE id=`models_metrics`.
  - Route stats (p95/ewma/hits/errors): `GET /state/route_stats`, SSE id=`route_stats`.
  - Env: `ARW_MODELS_METRICS_COALESCE_MS`, `ARW_MODELS_METRICS_PUBLISH_MS`, `ARW_ROUTE_STATS_COALESCE_MS`, `ARW_ROUTE_STATS_PUBLISH_MS` (manifest-driven).

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

- Keep the manifest under version control alongside other runtime configs so changes stay reviewable.
- The loader trims whitespace, deduplicates list entries, and warns on malformed durations instead of aborting startup.
- Planned sections remain in the spec to show intent, but only the keys listed in the loader table mutate runtime behavior today.
