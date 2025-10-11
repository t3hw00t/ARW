---
title: Memory Abstraction Layer
---

# Memory Abstraction Layer
Updated: 2025-10-11
Type: Explanation

Microsummary: The Memory Abstraction Layer (MAL) is the canonical schema and lifecycle for all memories (ephemeral, episodic, semantic, profile) in ARW. The new Memory Overlay Service builds on MAL to provide hybrid retrieval, explainable packing, and model-agnostic context delivery.

## Role inside ARW
- Acts as the single source of truth for agent memories inside the unified object graph (`memory_items`).
- Normalises provenance, durability, and trust metadata so every surface can reason about recall and retention.
- Powers the Memory Overlay Service, which exposes `memory.*` actions and feeds the context working set builder.
- Legacy `/memory/*` endpoints have been removed; the overlay exposes `/actions (memory.*)`, `/admin/memory/*` helpers, and `/state/memory` read-models instead.

## Canonical record
Every memory item shares the same canonical shape; lanes (ephemeral/episodic/semantic/profile) live inside metadata rather than separate tables.

| Field | Purpose |
| --- | --- |
| `id` (`uuid`) | Stable pointer used in context packs, links, and journal events. |
| `ts` (`unix_ms`) | Ingestion timestamp for recency scoring and TTL enforcement. |
| `agent_id`, `project_id` | Scope and tenancy controls; align with policy leases. |
| `kind` | `fact`, `obs`, `result`, `pref`, `plan`, `summary` – typed budgeting. |
| `text` | Human-readable memory excerpt or distilled summary. |
| `keywords`, `entities` | Optional JSON arrays used for lexical boosting and UI facets. |
| `source` | Structured provenance (`uri`, `tool`, `trace_id`). |
| `durability` | `ephemeral`, `short`, or `long`; informs TTL and recall boosts. |
| `trust` | Confidence 0.0–1.0 used for weighting results. |
| `privacy` | `private`, `project`, or `shared`; guards egress. |
| `ttl_s` | Optional override for GC cadence. |
| `links` | Graph edges to parents/children memories. |
| `extra` | Arbitrary adapter payload (e.g., `{ "lane": "episodic", "score": 0.42, "prob": 0.91 }`). |
| `vec` (derived) | Row in `memory_vec` / remote vector store used for ANN search. |

See [memory_overlay_service.md](memory_overlay_service.md#data-model) for full schema and indices.

### Hashing & dedupe
- MAL continues to hash `(agent_id, project_id, kind, text)` with SHA256 for dedupe and attribution; the hash is stored in `extra.hash`.

## Retrieval performance guardrails
- The `memory_records` table now carries `idx_mem_updated` and `idx_mem_lane_updated` indexes so the hot `ORDER BY updated DESC` scans stay on-disk sorted without temporary tables, maintaining steady-state lookup latency as the corpus grows.
- Hybrid retrieval trims candidate sorting to the requested limit using an unstable selection pass before the final ordering, avoiding O(n log n) sorts when callers only need the top slice of a large result set. This keeps working-set assembly responsive even with aggressive over-fetching.
- Embeddings persist in an `embed_blob` column (with the legacy JSON string kept as a fallback), so vector comparisons reuse pre-encoded little-endian floats instead of re-parsing text on every query.
- A background backfill task (`ARW_MEMORY_EMBED_BACKFILL_BATCH` / `ARW_MEMORY_EMBED_BACKFILL_IDLE_SEC`) upgrades legacy rows in place so existing deployments converge on the faster binary path without taking downtime.
- `memory.upsert` accepts `dedupe=true` to reuse existing IDs when the hash matches; events include `dedupe: true` for audit trails.

## Memory lanes & durability
| Lane | Default durability | Notes |
| --- | --- | --- |
| `ephemeral` | `ephemeral` | Scratchpad for the current turn; never packs into long-term context unless explicitly promoted. |
| `short_term` | `short` | Modular stack conversation buffer with 15-minute TTL (tunable via `ARW_MEMORY_SHORT_TTL_SECS`); mirrors episodic turns while loss metrics feed compression jobs. |
| `episodic` | `short` | Summaries of recent turns, tool outputs, and micro plans. |
| `semantic` | `long` | Durable facts, docs, source snippets; chunked + indexed. |
| `profile` | `long` | Preferences, API scopes, user/agent traits. |

Durability drives TTLs (minutes, hours, or months) and recency boosts during retrieval. Background janitors in `arw-memory-core` enforce expiry and publish `memory.item.expired` events.

## API surfaces
### Admin helpers
- `POST /admin/memory/apply` — convenience helper that inserts/updates memory items via the overlay.
- `GET /admin/memory` — quick snapshot of recent records (lane/limit filters); ideal for debugging.
- Quarantine endpoints: `GET /admin/memory/quarantine`, `POST /admin/memory/quarantine`, `POST /admin/memory/quarantine/admit`.

Legacy `/memory/*` routes have been removed; rely on the action-based flow below for all production-facing behavior.

### Memory Overlay actions (preferred)
- `memory.upsert` → Upsert item, update indices, emit `memory.item.upserted`.
- `memory.search` → Hybrid lexical/vector retrieval with RRF + MMR + scoring.
- `memory.pack` → Build context packs from ranked items using per-kind token budgets.

Every action is invoked via `POST /actions` and participates in the unified journaling/metrics pipeline. Responses include explainability payloads (scores, diversity decisions, token counts).

### Read-models & events
- `/state/memory` (SSE JSON Patch) shows incremental inserts, expirations, and latest pack preview per agent/project.
- Event topics: `memory.item.upserted`, `memory.item.expired`, `memory.pack.journaled`, `memory.overlay.metrics`.

## Retrieval & packing
- Candidate generation runs lexical (SQLite FTS or Tantivy) and vector (sqlite-vec or Qdrant) searches in parallel.
- Fusion uses Reciprocal Rank Fusion; diversity filtering applies Maximal Marginal Relevance with configurable similarity thresholds.
- Scoring blends RRF, recency, durability, and trust; boosts for `summary` and `plan` kinds keep strategic context present.
- Packing enforces a `PackBudget` with global `max_tokens` and per-kind ceilings (e.g., `summary:2`, `fact:6`).
- Token counting is model-aware via `TokenCounter` trait; default uses `tiktoken-rs` with heuristics fallback.

See [memory_overlay_service.md#retrieval-pipeline](memory_overlay_service.md#retrieval-pipeline) for the detailed algorithm and observability signals.

## Integration with other subsystems
- **Context Working Set**: `memory.pack` feeds `apps/arw-server/src/working_set.rs`; the working set loop can trigger additional packs when coverage is low.
- **Self-model & Belief graph**: semantic memories referencing entities feed the world model; `links.parents` connect to beliefs and artifacts.
- **Logic Units**: strategies can register custom packers or scoring tweaks by implementing the `ContextPacker` trait and providing pack presets.
- **Policy & Gating**: privacy scope and TTL drive policy checks before data leaves the node or remote collaborators request context.
- **Modular Cognitive Stack**: recall/compression/validation agents use MAL lanes as their single source of truth; orchestration contracts and provenance expectations are detailed in [Modular Cognitive Stack](modular_cognitive_stack.md).

## Migration status
| Phase | Status | Highlights |
| --- | --- | --- |
| Phase 0 | In progress | `arw-memory-core` crate carved out of `arw-kernel`; schema renamed to `memory_items` + vector map. |
| Phase 1 | Planned | Action handlers for `memory.upsert` / `memory.search`; `/state/memory` read-model scaffolding. |
| Phase 2 | Planned | `memory.pack`, retrieval fusion, token budgeting, working set integration. |
| Phase 3 | Future | Optional Tantivy/Qdrant backends, journaling tables, remote federation guardrails. |

Legacy docs and UI panels stay accurate because the new overlay reuses the same MAL data. Feature catalog entries now point at the overlay plan.

## Related documents
- [Memory Overlay Service](memory_overlay_service.md)
- [Memory Lifecycle](memory_lifecycle.md)
- [Context Working Set](context_working_set.md)
- [Object Graph](object_graph.md)
- [Durability](durability.md)

## Open items
- Finalise JSON Schema (`spec/schemas/memory_item.json`) once Phase 0 lands.
- Update SDKs and connectors to call the action interface; remove direct SQLite access in tooling.
- Decide whether to expose `memory.pack` as a Logic Unit hook for custom agents.
