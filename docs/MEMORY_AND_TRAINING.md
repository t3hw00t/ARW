---
title: Memory, Live Feedback & Conditional Training
---

# Memory, Live Feedback & Conditional Training
{ .topic-trio style="--exp:.7; --complex:.8; --complicated:.7" data-exp=".7" data-complex=".8" data-complicated=".7" }

Updated: 2025-09-06.

See also: [Feedback Engine](guide/feedback_engine.md)

## Objective

Memory is core, inspectable, and experimentable. Interactions show which memories are applied and why; training interactions can update deep/durable memory conditionally under policy.

## Terms

Entity: user, agent, tool, dataset, project, external service.

Memory layers: ephemeral, episodic, semantic, procedural.

Abstractions: summaries, exemplars, rules, graphs, embeddings, traces.

Dataset: versioned collection of memory records with provenance & policy tags.

Capsule: reproducible run bundle (prompts, tool calls, versions, events).

## UI Cross‑Reference
- In the Debug UI (`/debug`, set `ARW_DEBUG=1`), the Memory panel shows current memory and lets you apply, save, and load examples.
- Click the small “?” help next to Memory for a quick tip and a link back to this page.

## Live Memory Feedback (Probe)

Surfaces: CLI (--probe), Launcher panel, Debug UI overlay, VS Code peek.

Shows: selection summary, why-explanations, provenance, policy state, deltas.

Events: MemoryApplied, MemoryDelta, DatasetVersion (with trace/span ids).

## Conditional Training

Flow: TrainingRequest → policy/consent → TrainingPlan → commit → DatasetVersion → MemoryDelta.

Guardrails: policy categories (PII/public), human approvals, tests & regressions, size limits.

Modes: append exemplars; revise summaries; graph edits; vector upserts; procedural rule changes.

## Memory Lab (Experimentation)

Controls: dataset size/complexity, abstraction strategies, retrieval recipes, rule logic toggles.

Measures: latency, cost/tokens, accuracy, hallucination rate, stability.

Outputs: JSON/CSV/Parquet reports; OTel metrics; Debug UI visualizations.

## Data & Formats

MemoryRecord schema (JSON); TrainingRequest schema; versioned events (see /spec).

## APIs

Probe: GET /probe; SSE /events (subscribe to Memory*).

Training: POST /training/requests; /approve; /commit; /revert.

MCP tools for probe & training mirror HTTP.

## Interplay with Hardware & Governor

Probe/training emit/consume governor & pool events (GovernorChanged, PoolScaled).

Policy may deny deep updates under low-power profiles; capsules capture before/after probes.
