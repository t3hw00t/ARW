Agents running wild — Memory, Live Feedback & Conditional Training
Updated: 2025-09-06.

OBJECTIVE

Memory is core, inspectable, and experimentable. Interactions show which memories are applied and why; training interactions can update deep/durable memory conditionally under policy.

TERMS

Entity: user, agent, tool, dataset, project, external service.

Memory layers: ephemeral, episodic, semantic, procedural.

Abstractions: summaries, exemplars, rules, graphs, embeddings, traces.

Dataset: versioned collection of memory records with provenance & policy tags.

Capsule: reproducible run bundle (prompts, tool calls, versions, events).

UI cross‑reference
- In the Debug UI (`/debug`, set `ARW_DEBUG=1`), the Memory panel shows current memory and lets you apply, save, and load examples.
- Click the small “?” help next to Memory for a quick tip and a link back to this page.

LIVE MEMORY FEEDBACK (PROBE)

Surfaces: CLI (--probe), Launcher panel, Debug UI overlay, VS Code peek.

Shows: selection summary, why-explanations, provenance, policy state, deltas.

Events: MemoryApplied, MemoryDelta, DatasetVersion (with trace/span ids).

CONDITIONAL TRAINING

Flow: TrainingRequest → policy/consent → TrainingPlan → commit → DatasetVersion → MemoryDelta.

Guardrails: policy categories (PII/public), human approvals, tests & regressions, size limits.

Modes: append exemplars; revise summaries; graph edits; vector upserts; procedural rule changes.

MEMORY LAB (EXPERIMENTATION)

Controls: dataset size/complexity, abstraction strategies, retrieval recipes, rule logic toggles.

Measures: latency, cost/tokens, accuracy, hallucination rate, stability.

Outputs: JSON/CSV/Parquet reports; OTel metrics; Debug UI visualizations.

DATA & FORMATS

MemoryRecord schema (JSON); TrainingRequest schema; versioned events (see /spec).

APIS

Probe: GET /probe; WS /events (subscribe to Memory*).

Training: POST /training/requests; /approve; /commit; /revert.

MCP tools for probe & training mirror HTTP.

INTERPLAY WITH HARDWARE & GOVERNOR

Probe/training emit/consume governor & pool events (GovernorChanged, PoolScaled).

Policy may deny deep updates under low-power profiles; capsules capture before/after probes.
