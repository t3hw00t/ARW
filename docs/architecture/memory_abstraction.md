---
title: Memory Abstraction Layer
---

# Memory Abstraction Layer
Updated: 2025-09-15
Type: Explanation

Microsummary: A stable centerpoint for coherent thinking — unify episodic/semantic/procedural memory into abstract records with stable hashing, probabilistic value, and simple retrieval, backed by the kernel.

Goals
- Stable centerpoint: self‑image + identity anchor the agent’s reasoning across episodes.
- Abstract storage: records have `{ lane, kind, key, value, tags, hash, score, prob }`.
- Hashing: canonical hash over `{lane,kind,key,value}` for dedupe and attribution.
- Value/probability: `score` and `prob` fields capture utility and belief; calibrate with evaluation.
- Retrieval: fast LIKE search now; FTS/embeddings later; compose by lane and recency.

API (initial)
- `POST /memory/put` — put/merge a record; emits `memory.record.put`.
- `GET /state/memory/select?q=...&lane=...&limit=50[&mode=fts]` — retrieval; use `mode=fts` to search via FTS.
- `POST /memory/search_embed` — embedding search (cosine) over recent records with `embed` set.
- `POST /state/memory/select_hybrid` — hybrid selection that blends FTS hit, embedding similarity, recency (6h half‑life), and utility `score`.
- `POST /memory/link` — add a link `{ src_id, dst_id, rel?, weight? }`; emits `memory.link.put`.
- `GET /state/memory/links?id=...` — list outgoing links for a record.
- `POST /memory/select_coherent` — returns a coherent working set by taking hybrid seeds and expanding top links per seed.
- `POST /state/memory/explain_coherent` — like `select_coherent` but each item includes an `explain` block with component scores and link paths.

Kernel schema
- `memory_records`: `id`, `lane`, `kind`, `key`, `value(JSON)`, `tags`, `hash`, `score`, `prob`, `created`, `updated` with indexes.

Design notes
- Lanes correspond to earlier memory types (ephemeral/episodic/semantic/procedural) but MAL treats them uniformly.
- Hashing ensures dedupe and stable references; provenance attaches via events.
- FTS5 index is available for `mode=fts`; hybrid uses weights `0.5*sim + 0.2*fts + 0.2*recency + 0.1*utility`. Future: vector indexes, link graph (`memory_links`), learned weights.
  - Coherent expansion uses: `0.5*seed_score + 0.3*link_weight + 0.2*recency`. The `explain` block returns these parts for transparency.

See also: Context Working Set, Self‑Model, Logic Units, Evaluation Harness.
