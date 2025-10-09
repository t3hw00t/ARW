---
title: Modular Cognitive Stack
---

# Modular Cognitive Stack
Updated: 2025-10-09
Type: Explanation

Purpose
- Deliver a modular cognitive scaffold where specialized LLM agents collaborate under a single orchestrator, giving users responsive dialogue, durable memory, and self-checking outputs without sacrificing privacy.

Principles
- Separation of concerns: each agent owns a narrow role (dialogue, recall, compression, interpretation, validation, tooling) so prompts, latency budgets, and safety posture stay tunable per task.
- One source of truth: all agents read and write through the Memory Fabric so context cannot fork or drift.
- Measurable trust: every hand-off is typed, logged, and auditable; provenance accompanies answers and stored memories.
- Safe-by-default: agents operate behind leases and guardrails, and the orchestrator enforces policy before any tool or model runs.

Agents
- Chat Agent — user-facing dialogue layer that assembles replies from orchestrator-provided context and validation feedback.
- Memory Recall Agent — retrieves episodic, compressed, and curated memories from the Memory Fabric using hybrid (keyword + embedding + graph) search.
- Memory Compression Agent — distills new experiences into layered representations (episodic transcript, scoped summary, durable knowledge) with provenance and retention policies.
- Interpretation Agent — aligns recalled context with the current request, resolves conflicts, and produces structured briefs for the Chat Agent.
- Validation Agent — performs contradiction checks, guardrail evaluations, and “what-if” stress tests before a response ships or a memory is admitted.
- Tool Agent / Broker — mediates tool execution, captures inputs and outputs, and reports run metadata for provenance and downstream validation.
- Orchestrator Planner — (optional) evaluates outstanding goals, spins up specialist agents, and coordinates multi-turn plans when work spans several exchanges.

Orchestration Flow
1. Orchestrator receives a user event and emits a typed `conversation.turn` record into the Memory Fabric’s short-term buffer (`short_term` lane, ~15‑minute TTL) while mirroring the turn into episodic history for durable replay.
2. Recall Agent pulls relevant artifacts (episodic, compressed, knowledge) via the Memory Abstraction Layer and returns references + confidence scores.
3. Interpretation Agent fuses the recall set with request metadata, highlights conflicts, and generates a structured brief (`intent`, `context_refs`, `risks`, `open_questions`).
4. Chat Agent drafts a response using the brief and may request additional recall/interpretation passes if gaps remain.
5. Tool Agent executes any declared tools (`tool_id`, `input_payload`, `sandbox_requirements`) and streams results back through the orchestrator; Validation Agent can replay or spot-check critical calls.
6. Validation Agent runs safety, contradiction, and regression checks. On success it signs the response with provenance metadata; on failure it returns remediation hints or blocks delivery.
7. Compression Agent evaluates whether the turn should update durable memory (episodic log, summary stack, distilled knowledge) and writes accepted capsules with provenance and retention hints.
8. Orchestrator publishes telemetry (`turn.latency`, `memory.hit_rate`, `validation.status`) for observability dashboards and evaluation harnesses.

Message Contracts
- Agent messages use typed JSON envelopes with required fields: `intent`, `context_refs`, `evidence_ids`, `confidence`, `latency_budget_ms`, `policy_scope` (see `spec/schemas/modular_agent_message.json`).
- Payloads are agent-specific: chat responses require `text` + provenance citations, recall/compression agents declare structured item lists with scoring, validation agents emit `status` + findings, and orchestrator trainers surface goals and bundle hints. Unknown agents fall back to generic payloads but still carry lease envelopes.
- The server validation path enriches accepted envelopes with `payload_kind` and a `lifecycle` summary (lease scopes + validation gate) so Launcher's provenance lane can show whether human review or validation is still pending.
- Tool calls include `tool_id`, `operation_id`, `input_payload`, `sandbox_requirements`, `result`, and `evidence_id` for provenance linking (see `spec/schemas/modular_tool_invocation.json`).
- Tool broker submissions now carry a `policy_scope` (leases + declared capabilities); the kernel enforces sandbox-derived capability requirements before accepting an invocation and records the enriched provenance for Launcher and Evaluation Harness consumers.
- Schemas live in the shared registry (see [API and Schema](../API_AND_SCHEMA.md)) and are versioned; orchestrator rejects payloads that fail validation.
- Server actions `modular.agent_message` and `modular.tool_invocation` already perform schema validation plus active-lease checks so specialists can be exercised safely during development.
- Successful validations emit `modular.agent.accepted` and `modular.tool.accepted` bus events so dashboards and provenance panes can surface outcomes without parsing generic action logs.
- Prometheus exports `arw_modular_agent_total` and `arw_modular_tool_total` counters, while `/metrics` JSON includes aggregated agent/tool counts for quick dashboards.

Memory Fabric
- Short-term buffer: per-conversation cache managed by the orchestrator for rapid back-references and validation inputs.
- Episodic store: append-only log of turns and tool runs with corr_id stitching; feeds the Training Park and replay tools.
- Compression tiers: layered summaries (extract → abstract → outline) tagged with source IDs, freshness timestamps, and compression loss scores.
- Distilled knowledge base: normalized facts and procedures with confidence and applicability tags for cross-project reuse.
- Storage backends plug into the [Memory Abstraction Layer](memory_abstraction.md); they support hybrid search, TTL hygiene, and policy-driven retention.

Governance & Safety
- Privacy: PII scrubbing before durable storage, configurable retention windows, and encryption-at-rest when the Memory Fabric runs off-device.
- Policies: leases scope which agents and tools can read/write memory lanes; orchestrator audits every cross-lane access (see [Capability & Consent Ledger](capability_consent_ledger.md)).
- Compliance: data residency tags and audit export endpoints integrate with the existing governance pack.
- Guardrails: Validation Agent enforces tool output moderation, prompt-injection detection, and contradiction checks against stored beliefs (see [Lightweight Mitigations](lightweight_mitigations.md)).

Reliability & Operations
- Health checks per agent with timeouts, exponential backoff, and fallback paths (e.g., use cached summaries if Recall Agent is unavailable).
- Structured logging with corr_id for every turn; traces cover agent hops, tool runs, and memory mutations (refer to [Observability (OTel)](observability_otel.md)).
- Graceful degradation: orchestrator downshifts to a single-agent flow when specialists are offline, flagging reduced assurance to the user.
- Deployment: each agent runs in its own container or process slot with resource budgets; orchestrator auto-scales workers based on queue depth and latency targets.
- Upgrade safety: staged rollout with shadow traffic, persisted compat matrices for message schemas, and feature flags to toggle specialists per project before global rollout.

Accessibility & UX
- Validation outcomes, memory evidence, and tool traces surface in sidecars with text + icon affordances, aria-labels, and status summaries consumable by screen readers.
- Planner/agent lineage timelines expose keyboard-navigation, captioned tooltips, and transcript exports so co-drivers can audit turns without inspecting raw logs.
- User preferences (language, contrast, accommodations) flow into agent prompts as context so responses honour accessibility choices, and compression tiers respect redaction flags.

Evaluation & Metrics
- Offline harness: run curated prompts through the stack to measure `response_quality`, `memory_hit_rate`, `compression_loss`, `validation_catch_rate`, and `tool_accuracy`.
- Online telemetry: dashboards track per-turn latency, agent backlogs, provenance coverage, and the percentage of responses blocked or revised by Validation.
- AB testing: orchestrator routes a slice of traffic through experimental agent configurations; results feed the [Experiment Orchestrator](experiment_orchestrator.md).

Extensibility & Roadmap
- Agent registry: specialists advertise capabilities, required inputs, and policy scopes so the orchestrator can discover and schedule them dynamically.
- Plugin protocol: third parties can ship agents or tools that declare schemas, safety notes, and runtime requirements; orchestrator negotiates version compatibility.
- Roadmap phases align with [Roadmap → Modular Cognitive Stack](../ROADMAP.md#priority-two--modular-cognitive-stack--memory-orchestration) and backlog items under the Never-Out-Of-Context stream.
- Future additions include multimodal recall/compression, simulation agents for “what-if” stress tests, and adaptive planners tuned by the Evaluation Harness.

Touchpoints
- Builds on [Agent Orchestrator](agent_orchestrator.md) for training and promotion of specialist agents.
- Shares storage and hygiene with the Memory Abstraction Layer, Memory Lifecycle, and Context Working Set documents.
- Coordinates with the managed runtime supervisor for model selection and lease enforcement.
- Surfaces context and provenance in UI flows covered by [Workflow Views & Sidecar](../guide/workflow_views.md) and [UI Architecture Options](ui_architecture_options.md).
