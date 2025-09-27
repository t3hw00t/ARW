---
title: Runtime Matrix
---

# Runtime Matrix
Updated: 2025-09-27
Type: How‑to

Grid of Models × Hardware × Sandboxes with live health/throughput and load. Pin preferred combos per agent/project; surface degradations as events.

Matrix cells
- Model: id, size, quantization
- Hardware: CPU/GPU/NPU; memory/VRAM
- Sandbox: native|container|WASI; isolation level
- Node: derived from `ARW_NODE_ID` or hostname (stable per machine)
- Health: ok/degraded/error; latency p50/p95; throughput; errors; error‑rate

Features
- Pin combos per agent/project; fallbacks with policy
- Event‑driven: `runtime.health`, `models.changed`, `policy.*`
- Quick actions: (re)load model, switch runtime/sandbox, open logs
- Accessibility: status summaries ship with aria hints covering degradation reasons

Backends
- Local engines (llama.cpp/Ollama/whisper/faster‑whisper)
- Optional remote fallback (opt‑in)

Policy
- Capabilities per cell (gpu, sandbox:<kind>) with TTL leases

Telemetry payloads now include aggregated HTTP error counts, request totals, and derived error rates so dashboards and alerts can distinguish between latency spikes and outright failures. The local publisher caches the resolved node identifier to avoid repeated hostname lookups during steady-state operation.

See also: Performance & Reasoning Playbook for how the scheduler selects Quick/Balanced/Deep/Verified against SLOs and budgets.
