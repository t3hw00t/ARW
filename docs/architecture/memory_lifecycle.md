---
title: Memory Lifecycle
---

# Memory Lifecycle

Updated: 2025-09-19
Type: Explanation

Memory is a first-class surface in the unified server: the kernel (via `arw-memory-core`) stores layered memories, the `memory.*` actions manage them, and `/state/memory` patches plus events keep UIs and tools in sync. This page folds in the legacy `Memory & Training` content so we do not lose important details as the overlay service replaces the older `/memory/*` REST endpoints.

## Goals
- Keep every memory item inspectable with provenance and policy tags.
- Blend layered memories (ephemeral -> episodic -> semantic -> procedural) into a just-in-time working set.
- Allow conditional training with approvals so improvements stay reproducible.
- Surface live feedback (why a memory was chosen, what changed) everywhere.

## Terms
- **Memory layer**: category such as `ephemeral`, `episodic`, `semantic`, or `procedural`.
- **Capsule**: reproducible run bundle (prompts, tool calls, versions, events) emitted by the kernel.
- **Dataset**: versioned collection of memory records with provenance and policy tags.
- **Working set**: the assembled context slice served by `/context/assemble`.

## Mounts & Retention
Memory mounts declare retention policy, dedup keys, freshness windows, and GC rules. Periodic distill/summarize passes collapse long-tail history into structured notes so hot context stays lean.

Policies propagate from inputs to memories and artifacts (`private`, `project`, `shared`, with legacy `public/internal/secret/regulated` values translated during migration). Redact/forget flows purge memories, caches, and snapshots by classification.

Quality metrics (recall, diversity, coverage) feed dashboards in Training Park and populate `state/memory_metrics` as those read-models ship.

## Live Memory Feedback
- **Surfaces**: CLI (`--probe`), the legacy Debug UI (`/debug` with `ARW_DEBUG=1`), launcher overlays, and future unified UIs subscribe to `memory.*` events.
- **Signals**: `memory.applied`, `memory.delta`, `memory.dataset.version`, and working-set events explain which items landed and why.
- **Status**: The unified server already emits the same events; the UI portion is still provided by the legacy service until the new sidecar lands.

## Conditional Training
Flow: `TrainingRequest` -> policy/consent review -> `TrainingPlan` -> commit -> `DatasetVersion` -> `MemoryDelta`.

Guardrails:
- Policy categories (PII/public/etc.) and leases gate deep updates.
- Human approvals and regression checks are required before promoting durable memories.
- Size limits and coverage thresholds keep updates bounded.

Modes include exemplar append, summary refresh, graph edits, vector upserts, and procedural rule changes. MCP tools mirror the HTTP APIs so both local and remote agents can participate under policy.

## Memory Lab (Experimentation)
Controls dataset size/complexity, abstraction strategies, retrieval recipes, and rule toggles. Measurements cover latency, cost/tokens, accuracy, hallucination rate, and stability. Outputs ship as JSON/CSV/Parquet plus structured events for dashboards.

## Data & Formats
Key schemas live in `spec/schemas/`:
- `memory_item.json` (new overlay schema replacing `memory_record.json`)
- `training_request.json`
- `memory_delta.json`

`memory_item.json` mirrors the canonical fields described in [Memory Abstraction Layer](memory_abstraction.md) and [Memory Overlay Service](memory_overlay_service.md). `memory_record.json` remains available for backward compatibility until downstream clients migrate.

Run `python3 scripts/gen_feature_catalog.py` after schema updates to keep the interface catalog aligned.

## APIs & Events
- Actions (`POST /actions`): `memory.upsert`, `memory.search`, `memory.pack` are the preferred interface. They emit `memory.item.upserted`, `memory.item.expired`, and `memory.pack.journaled` topics.
- Legacy REST (still supported for now): `/memory/put`, `/memory/search`, `/memory/search_embed`, `/state/memory/select`, `/memory/link`, `/memory/select_coherent`, `/state/memory/explain_coherent` – these wrap the action handlers and will be retired in the Phase 3 window.
- `/state/memory` SSE view exposes the latest items, expirations, and packed context previews as JSON Patch deltas. Legacy `/state/memory_*` routes redirect here.
- `/context/assemble` and `/context/rehydrate` consume `memory.pack` internally and keep their streaming contract intact.
- `/training/*` endpoints are being ported to the unified server; until they land, the executor remains internal and episodes capture resulting capsules.

## Interplay with Hardware & Governor
The governor publishes `GovernorChanged` events that memory planners listen to (for example, reducing expansion depth on low-power profiles). Hardware probes inform offload choices for embedding search and vector workloads.

## Legacy Coverage
`memory.probe`, `feedback.evaluate`, and `feedback.apply` tooling now run on the unified server (tool IDs match previous bridge behaviour). The new sidecar will surface these flows without the legacy UI.

See also: Context Working Set, Data Governance, Context Recipes, Training Park.
