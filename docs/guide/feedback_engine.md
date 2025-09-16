---
title: Feedback Engine (Lightweight, Near‑Live)
---

# Feedback Engine (Lightweight, Near‑Live)
{ .topic-trio style="--exp:.5; --complex:.7; --complicated:.6" data-exp=".5" data-complex=".7" data-complicated=".6" }

The feedback engine observes service events and route metrics to propose gentle tuning suggestions (e.g., HTTP timeout hints, memory ring size), without blocking request paths.

Updated: 2025-09-16
Type: How‑to

## Goals
- Extremely light: constant memory, O(1) updates per event, periodic evaluation off the hot path.
- Near‑live: publishes `feedback.suggested` every ~250–500 ms when changes occur; UIs update via SSE.
- Safe by default: suggestions only; applies are policy‑gated and rate‑limited.

## Runtime
- Engine cadence: `ARW_FEEDBACK_TICK_MS` (ms) or `tick_ms` in [`configs/feedback.toml`](https://github.com/t3hw00t/ARW/blob/main/configs/feedback.toml) (default 500).
- Suggestions include `id`, `action` (`hint`, `mem_limit`, `profile`), `params`, `rationale`, and `confidence`.
- Live view: SSE `/admin/events` with `feedback.suggested`, or `GET /feedback/suggestions`.

## Policy (Guardrails)
- Caps and bounds are merged from [`configs/feedback.toml`](https://github.com/t3hw00t/ARW/blob/main/configs/feedback.toml) and env vars:
  - `ARW_FEEDBACK_APPLY_PER_HOUR` (default 3)
  - `ARW_FEEDBACK_HTTP_TIMEOUT_MIN/MAX` (default 5..=300)
  - `ARW_FEEDBACK_MEM_LIMIT_MIN/MAX` (default 50..=2000)
- Effective policy: `GET /feedback/policy` returns the current values.
- Applies are rejected with a clear reason if caps/bounds are exceeded.

## Config File: [`configs/feedback.toml`](https://github.com/t3hw00t/ARW/blob/main/configs/feedback.toml)
```toml
# tick_ms = 500
# apply_per_hour = 3
# http_timeout_min = 5
# http_timeout_max = 300
# mem_limit_min = 50
# mem_limit_max = 2000
```

## APIs
- `GET /feedback/suggestions` → { version, suggestions }
- `GET /feedback/updates?since=N` → 204 if unchanged
- `POST /feedback/apply` → { ok, reason? } (policy‑gated)
- `GET /feedback/policy` → effective caps/bounds

## Specs
- OpenAPI: `/spec/openapi.yaml`
- AsyncAPI (events): `/spec/asyncapi.yaml` (includes feedback.* channels)
- MCP tools catalog: `/spec/mcp-tools.json`

## UI (Debug)
- Near‑live list with confidence badges and Apply buttons.
- Policy bounds/caps displayed inline; toasts on success/failure.

## Notes
- Keep `ARW_DEBUG=1` for local development; secure admin endpoints with `ARW_ADMIN_TOKEN` otherwise.
- For heavy loads, the engine drops/samples events rather than blocking; consumers can resync via `GET /feedback/suggestions`.
