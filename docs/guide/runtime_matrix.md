---
title: Runtime Matrix
---

# Runtime Matrix
Updated: 2025-09-27
Type: Blueprint
Status: In progress

ARW seeds a runtime matrix read-model from `runtime.health` events. Today it reports per-node health derived from HTTP telemetry; the full grid of models and hardware remains under active development.

## Current state
- Local node health published every five seconds with latency/error summaries drawn from server metrics.
- Node identifiers resolve from `ARW_NODE_ID` (or fallback hostname) and feed the runtime matrix read-model.
- Accessibility strings accompany each status so dashboards can surface the same context.

## Roadmap
- Grid of Models × Hardware × Sandboxes with live health/throughput and load per cell.
- Pin preferred combos per agent/project and fall back automatically under policy control.
- Event-driven updates across `runtime.health`, `models.changed`, and `policy.*` topics.
- Quick actions: (re)load model, switch runtime/sandbox, open logs.
- Capability-aware leases per cell (gpu, sandbox:<kind>) with TTL expirations.
- Backends: local llama.cpp, planned ONNX Runtime/vLLM/whisper adapters, and opt-in remote fallbacks.

Telemetry payloads will continue to grow: aggregated HTTP error counts, request totals, derived error rates, and slow-route annotations help dashboards distinguish latency spikes from outright failures while the matrix expands to multi-runtime tracking.

See also: Performance & Reasoning Playbook for how the scheduler selects Quick/Balanced/Deep/Verified against SLOs and budgets.
