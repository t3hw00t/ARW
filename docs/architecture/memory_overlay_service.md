---
title: Memory Overlay Service
---

# Memory Overlay Service
Updated: 2025-10-09
Status: Planned
Type: Explanation

Microsummary: Layered, LLM-agnostic memory service that sits on the unified object graph, exposes `memory.*` actions, and feeds the context working set with budget-aware packs built from hybrid retrieval over SQLite+vec0.

## Why this exists
- Make memory a first-class, swappable subsystem inside the unified server instead of a grab-bag of kernel helpers.
- Keep every model/agent compatible: a thin overlay adapts memory into model-sized context packs with token budgets and quotas per item kind.
- Preserve ARW’s local-first posture (SQLite) while leaving room for remote/backplane indexes like Qdrant or Tantivy clusters later on.

## Goals
- **Consistent API surface**: expose `memory.upsert`, `memory.search`, and `memory.pack` as lightweight actions on `/actions`, plus `/state/memory` as a live SSE read-model.
- **Layered memories**: track ephemeral, episodic, semantic, profile, and story-thread lanes with durability metadata while storing them in one canonical table.
- **Threaded recall**: maintain topic-weighted story threads so assistants can recover the narrative spine of an initiative without replaying every turn.
- **Deterministic context build**: hybrid lexical/vector retrieval fused via RRF, diversified via MMR, then budgeted into context packs with per-kind quotas.
- **Explainable recall**: emit journaling metadata for every packed item (scores, boosts, lanes) so downstream UIs can replay why an item landed.
- **Composable adapters**: keep embedding generation, token counting, and packing strategies pluggable so different agents/models can reuse the same store.

## High-level architecture
```

When `topics` are provided the overlay normalises each hint, tags the memory (`topic:<slug>`), and fans out updates to the `story_thread` lane so follow-up retrieval can pivot straight to the relevant narrative thread.
┌────────────────────────────────────────────────────────────────────────────┐
│ arw-server                                                                 │
│                                                                            │
│  ┌────────────────────┐  actions  ┌──────────────────────────────┐         │
│  │ /actions router    │──────────▶│ Memory Action Handlers       │         │
│  └────────────────────┘           │  - memory.upsert             │         │
│                                   │  - memory.search             │         │
│                                   │  - memory.pack               │         │
│                                   └───────────────┬──────────────┘         │
│                                                   │                       │
│                                           ┌───────▼────────┐               │
│                                           │ arw-memory-core│               │
│                                           │  (SQLite store) │               │
│                                           └───────┬────────┘               │
│                                                   │                       │
│                                  ┌────────────────▼───────────┐            │
│                                  │ arw-memory-retrieval       │            │
│                                  │  - lexical (FTS/Tantivy)   │            │
│                                  │  - vector (sqlite-vec)     │            │
│                                  │  - RRF + MMR + scoring     │            │
│                                  │  - pack budgeting          │            │
│                                  └──────────┬────────────────┘            │
│                                             │                             │
│                             ┌───────────────▼───────────────┐              │
│                             │ Embedding adapters            │              │
│                             │ (default: arw-embeddings-     │              │
│                             │  fastembed, swappable)        │              │
│                             └───────────────────────────────┘              │
│                                                                            │
│  ┌──────────────────────────────────────────────────────────────────────┐  │
│  │ /state/memory read-model                                             │  │
│  │  - live patch stream (JSON Patch / SSE)                              │  │
│  │  - publishes item deltas + pack previews                             │  │
│  └──────────────────────────────────────────────────────────────────────┘  │
│                                                                            │
└────────────────────────────────────────────────────────────────────────────┘
```

## Data model
Canonical row stored in SQLite, designed for portability and provenance.

```sql
CREATE TABLE IF NOT EXISTS memory_items (
  id TEXT PRIMARY KEY,
  ts INTEGER NOT NULL,
  agent_id TEXT NOT NULL,
  project_id TEXT NOT NULL,
  kind TEXT NOT NULL,             -- fact | obs | result | pref | plan | summary
  text TEXT NOT NULL,
  keywords TEXT,                  -- JSON array string
  entities TEXT,                  -- JSON array string
  source TEXT,                    -- JSON blob (uri/tool/trace_id)
  durability TEXT NOT NULL,       -- ephemeral | short | long
  trust REAL NOT NULL DEFAULT 0.5,
  privacy TEXT NOT NULL DEFAULT 'private',
  ttl_s INTEGER,
  links TEXT,                     -- JSON { parents: [], children: [] }
  extra TEXT                      -- JSON catch-all for adapters
);

.load ./vec0
CREATE VIRTUAL TABLE IF NOT EXISTS memory_vec
USING vec0(embedding float[384]);

CREATE TABLE IF NOT EXISTS memory_vec_map (
  id TEXT PRIMARY KEY,
  rowid_vec INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_keywords (
  keyword TEXT NOT NULL,
  id TEXT NOT NULL,
  PRIMARY KEY(keyword, id)
);

CREATE INDEX IF NOT EXISTS idx_memory_ts ON memory_items(ts DESC);
CREATE INDEX IF NOT EXISTS idx_memory_kind ON memory_items(kind);
CREATE INDEX IF NOT EXISTS idx_memory_privacy ON memory_items(privacy);
CREATE INDEX IF NOT EXISTS idx_memory_agent_project
  ON memory_items(agent_id, project_id, ts DESC);
```

### Derived indices
- **Lexical**: start with SQLite FTS5; feature-flag Tantivy (`arw-memory-core` gets a `tantivy` feature that mirrors the FTS feed).
- **Vector**: sqlite-vec by default; map rowids through `memory_vec_map`. Flag `--memory-vector-backend=qdrant` later for remote HNSW.

### Item schema (JSON view)
```json
{
  "id": "uuid",
  "ts": 1736886865123,
  "agent_id": "hub://agents/planbot",
  "project_id": "hub://projects/demo",
  "kind": "summary",
  "text": "Condensed findings about ...",
  "keywords": ["pricing", "comp"],
  "entities": [{"type": "company", "value": "Acme"}],
  "source": {"uri": "file:///...", "tool": "crawl", "trace_id": "..."},
  "durability": "long",
  "trust": 0.8,
  "privacy": "project",
  "ttl_s": 7_776_000,
  "links": {"parents": ["..."], "children": ["..."]},
  "vec": {"dim": 384, "ref": 9345},
  "extra": {"lane": "episodic", "tags": ["sprint-17"]}
}
```

## Actions and surfaces
All new endpoints ship via the unified action bus (`/actions`) and the SSE state stream.

### `memory.upsert`
- Request: `{ "input": MemoryUpsertInput }`
- Behavior: upsert canonical row, update keyword/vec indices, emit `memory.item.upserted` topic, patch `/state/memory`.
- Supports dedupe via SHA256 of `(agent_id, project_id, kind, text)` when `input.dedupe == true`.

```json
{
  "kind": "memory.upsert",
  "input": {
    "agent_id": "hub://agents/planbot",
    "project_id": "hub://projects/demo",
    "kind": "fact",
    "text": "Widget Pro launched Q4 FY24.",
    "durability": "short",
    "keywords": ["widget", "launch"],
    "entities": [{"type":"product","value":"Widget Pro"}],
    "source": {"uri": "file:///notes.md", "tool":"note_taker"},
    "ttl_s": 86_400,
    "privacy": "project",
    "embedding": {"hint": "fastembed:e5", "vector": [ ... ]},
    "links": {"parents": ["mem://summary:123"], "children": []},
    "topics": [{"name": "launch validation", "weight": 0.9}],
    "extra": {"lane": "episodic"}
  }
}
```

### `memory.search`
- Inputs: free-text query, optional embedding, filters (project, agent, durability, kinds), limits for lexical (`k1`) and vector (`k2`).
- Output: fused results with component scores, plus RRF/MMR metadata for transparency.

```json
{
  "kind": "memory.search",
  "input": {
    "query": "pricing model changes",
    "project_id": "hub://projects/demo",
    "filters": {"kinds": ["summary", "fact"], "privacy": ["project", "shared"]},
    "limits": {"lexical": 12, "vector": 24},
    "embedding": {"vector": [...]} 
  }
}
```

### `memory.pack`
- Inputs: query + `PackBudget { max_tokens, per_kind_caps }` and `model` identifier (to choose token counter and pack style).
- Output: `{ "items": [...], "package": {"text": "...", "tokens": 924, "explain": [...] }}`.
- Each item carries `decision_trace`: RRF rank, MMR retained/dropped reason, boosts, recency decay, and slot assignment.

## Retrieval pipeline
1. **Candidate generation**
   - Lexical: `memory_core.lexical(query, filters, k1)` (FTS or Tantivy).
   - Vector: `memory_core.vector(embedding, filters, k2)` using sqlite-vec.
   - Optional heuristics (recency-only, pinned summaries, per-kind minimums).
2. **Fusion**
   - Apply Reciprocal Rank Fusion with tunable weights (defaults `.7` lexical, `.3` vector).
   - Normalize with per-kind bonuses (e.g., `summary` +0.05, `plan` +0.03).
3. **Diversity filter**
   - Maximal Marginal Relevance with cosine distance over embeddings.
   - Drop near-duplicates; guarantee representation across durability tiers.
4. **Scoring**
   - Base: RRF score.
   - Boosts: recency decay (half-life 6h for short, 7d for long), trust multiplier, manual pin tags.
5. **Pack budgeting**
   - Token counter per model (via `TokenCounter` trait).
   - Hard slots: instructions/safety/reserved (from budget config).
   - Soft slots: fill by `per_kind_caps` descending by priority weights.
6. **Explainability**
   - For each item: include `scores`, `diversity_drop?`, `token_cost`, `pack_slot`.
   - Emit `memory.pack.journaled` event for debugging.

## Integration points
- **arw-memory-core crate**
  - Owns schema migrations, CRUD, lex+vec adapters, TTL GC, journaling.
  - Exposes async methods returning structured DTOs used by server and background jobs.
- **arw-memory-retrieval crate**
  - Houses fusion, scoring, budgeter, and packer traits.
  - Reuses `arw-topics` to publish events, `arw-core` for error types.
- **arw-embeddings-fastembed crate**
  - Thin wrapper around `fastembed` ONNX runtime; optional (feature `fastembed`).
  - Provides default implementation of `EmbeddingAdapter` trait.
- **apps/arw-server**
  - `api_actions.rs`: register new action handlers.
  - `read_models.rs`: add `/state/memory` patch emitter.
  - `story_threads.rs`: normalise `topics` hints and keep `story_thread` summaries + graph links fresh.
  - `working_set.rs`: switch to `ContextPacker` trait for assembling context.
  - `metrics.rs`: new histograms (`memory.retrieval.latency_ms`, `memory.pack.tokens`).
- **arw-kernel**
  - Deprecate inline memory SQL; wrap the new crates.
  - Legacy `/memory/*` shims have been removed from `arw-server`; only `/admin/memory/*` helpers remain for debugging.
- **Context working set**
  - Replace ad-hoc queries with `memory.pack` under the hood.
  - Keep iterative CRAG flow: if coverage alarms fire, re-run `memory.pack` with widened budget.

## Observability & privacy
- Emit timings for each pipeline stage (`lexical_ms`, `vector_ms`, `fusion_ms`, `pack_ms`).
- Count hits per durability/kind to track coverage.
- Honor `privacy` scope: default filter to `private`+`project`; require explicit override to retrieve `shared`.
- Hygiene loop enforces TTLs and lane caps: expired records and overflowed lanes are reclaimed (`memory.item.expired` events), Prometheus counters (`arw_memory_gc_expired_total`, `arw_memory_gc_evicted_total`) track reclaimed rows, and `/state/memory` is patched live so operators see the updated snapshot.
- Respect egress firewall: remote adapters only allowed once guard approves.

## Implementation roadmap

### Phase 0 – Prep & carve-out (1 PR)
- Add `arw-memory-core` crate with migrated schema/methods from `arw-kernel` but no API changes.
- Wire unit tests to cover SQLite migrations and lane-specific TTLs.
- Update `arw-kernel` to delegate to the new crate behind feature flag `memory_overlay` (default on).

### Phase 1 – Overlay actions (1-2 PRs)
- Implement action handlers in `api_actions.rs` for `memory.upsert` & `memory.search` using new crate.
- Create skeletal `/state/memory` read-model streaming inserts and expirations.
- Add topic constants (`memory.item.upserted`, `memory.pack.journaled`).
- Update docs + feature catalog to mark overlay service as "beta".

### Phase 2 – Retrieval + packer (multiple PRs)
- Build `arw-memory-retrieval` with fusion + budgeter traits.
- Implement `memory.pack` action and integrate with `working_set` builder.
- Provide default token counter using `tiktoken-rs` and fallback to heuristics when unknown.
- Surface context packs in UI sidecar (reuse existing working-set SSE stream).

### Phase 3 – Optional backends & performance (stretch)
- Feature-gate Tantivy lexical index and Qdrant adapter.
- Add background job for FastEmbed batch generation; fall back to on-demand embedding.
- Introduce journaling table for pack decisions (SQLite `memory_pack_journal`).

### Migration + compatibility
- Legacy `/memory/*` REST endpoints have been removed from `arw-server`; rely on `/actions (memory.*)` and `/admin/memory/*` helpers instead.
- Provide schema upgrade script (Rust migration + SQL) to move from `memory_records` to `memory_items`.
- Update `memory_abstraction.md` and `memory_lifecycle.md` to explain the overlay; highlight `/admin/memory/*` helpers instead of legacy routes.

## Related documents
- [architecture/memory_abstraction.md](memory_abstraction.md) – conceptual lanes & hashing (to be aligned with this plan).
- [architecture/memory_lifecycle.md](memory_lifecycle.md) – lifecycle controls.
- [architecture/context_working_set.md](context_working_set.md) – context budgeting; `memory.pack` feeds this pipeline.

## Open questions
1. How aggressively should we GC vector rows when TTL expires? (Probably in the same transaction as item removal.)
2. Do we need a dedicated `memory_profiles` table for user prefs, or can we keep them in `memory_items` with durability=`long`? (Lean toward keeping canonical table simple first.)
3. Should `memory.pack` allow pushing artifacts (files/snippets) inline, or only references? (Start with references + short excerpts.)
4. What telemetry granularity is acceptable for the default lightweight build (avoid heavy histograms on low-power devices)?

## Next actions
- Land Phase 0 PR (crate carve-out + delegation).
- Draft end-to-end tests proving `memory.pack` respects per-kind quotas and token limits.
- Update CLI/SDK consumers to call the action endpoints (`memory.*`) or the `/admin/memory/*` helpers when diagnosing issues.
