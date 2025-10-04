---
title: Runtime Matrix
---

# Runtime Matrix
Updated: 2025-10-02
Type: Blueprint
Status: In progress

ARW seeds a runtime matrix read-model from `runtime.health` events. Today it merges per-node HTTP telemetry with runtime registry states and accelerator summaries while the full grid of models and hardware remains under active development.

## Current state
- Local node health published every five seconds; payloads now merge HTTP telemetry with the runtime registry snapshot so readiness, degraded/error counts, and restart pressure all travel together.
- Read-model payloads include `ttl_seconds` so dashboards know exactly how long a snapshot should be treated as fresh before polling or prompting for an updated heartbeat.
- Accelerator rollups highlight CPU/GPU/NPU availability and the state mix per accelerator so operators can spot when a GPU lane degrades or drops offline.
- Node identifiers resolve from `ARW_NODE_ID` (or fallback hostname) and feed the runtime matrix read-model.
- Accessibility strings accompany each status so dashboards can surface the same context.
- Restart budgets surface remaining automatic restarts, the configured window, and the reset horizon so operators can decide when to intervene or widen the budget.
- Launcher now mirrors the snapshot with a header badge in Project Hub, highlighting readiness counts, restart headroom, and next reset.
- CLI shortcuts: `arw-cli runtime status` prints the same snapshot (or `--json` for raw), and `arw-cli runtime restore --id <runtime>` triggers supervised restores while echoing the remaining budget or budget exhaustion.
  - Text mode now reports the active `ttl_seconds` so operators know when the matrix snapshot should be considered stale.
  - JSON mode emits `{ "supervisor": ..., "matrix": ... }` so scripts can consume both views (including `ttl_seconds`) in one call.
- Smoke check: `just runtime-smoke` launches a stub llama endpoint, points the server at it, and verifies `chat.respond` flows end-to-end without needing model weights (extend with MODE=real once hardware-backed smoke rigs land). The helper exits automatically after `RUNTIME_SMOKE_TIMEOUT_SECS` seconds (defaults to the shared `SMOKE_TIMEOUT_SECS`, falling back to 600). Set either knob to `0` to disable the guard during manual debugging.
  - To exercise a real llama.cpp build: `MODE=real LLAMA_SERVER_BIN=/path/to/server LLAMA_MODEL_PATH=/path/to/model.gguf just runtime-smoke`. Optionally pass `LLAMA_SERVER_ARGS="--your --flags"` or `LLAMA_SERVER_PORT=XXXX` to match your deployment.

## Roadmap
- Grid of Models × Hardware × Sandboxes with live health/throughput and load per cell.
- Pin preferred combos per agent/project and fall back automatically under policy control.
- Event-driven updates across `runtime.health`, `models.changed`, and `policy.*` topics.
- Quick actions: (re)load model, switch runtime/sandbox, open logs.
- Capability-aware leases per cell (gpu, sandbox:<kind>) with TTL expirations.
- Backends: local llama.cpp, planned ONNX Runtime/vLLM/whisper adapters, and opt-in remote fallbacks.

Telemetry payloads will continue to grow: aggregated HTTP error counts, request totals, derived error rates, slow-route annotations, runtime-state rollups, and accelerator summaries help dashboards distinguish latency spikes from outright failures while the matrix expands to multi-runtime tracking.

See also: Performance & Reasoning Playbook for how the scheduler selects Quick/Balanced/Deep/Verified against SLOs and budgets.
