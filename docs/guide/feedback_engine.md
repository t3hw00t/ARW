---
title: Feedback Engine (Lightweight, Near‑Live)
---

# Feedback Engine (Lightweight, Near‑Live)
{ .topic-trio style="--exp:.5; --complex:.7; --complicated:.6" data-exp=".5" data-complex=".7" data-complicated=".6" }

The feedback engine observes service events and route metrics to propose gentle tuning suggestions (e.g., HTTP timeout hints, memory ring size), without blocking request paths.

Updated: 2025-09-20
Type: How‑to

## Goals
- Extremely light: constant memory, O(1) updates per event, periodic evaluation off the hot path.
- Near‑live: publishes `feedback.suggested` every ~250–500 ms when changes occur; deltas stream via `feedback.delta` so reviewers can see what changed between versions.
- Safe by default: suggestions only; applies are policy‑gated and rate‑limited.

## Runtime
- Engine cadence: `ARW_FEEDBACK_TICK_MS` (ms) or `tick_ms` in [`configs/feedback.toml`](https://github.com/t3hw00t/ARW/blob/main/configs/feedback.toml) (default 500).
- Suggestions include `id`, `action` (`hint`, `mem_limit`, `profile`), `params`, `rationale`, and `confidence`.
- Live view: SSE `/events` with `feedback.suggested` and `feedback.delta`, or `GET /admin/feedback/suggestions` / `GET /admin/feedback/state` (which now includes a capped `delta_log`).

## Policy (Guardrails)
- Caps and bounds are merged from [`configs/feedback.toml`](https://github.com/t3hw00t/ARW/blob/main/configs/feedback.toml) and env vars:
  - `ARW_FEEDBACK_APPLY_PER_HOUR` (default 3)
  - `ARW_FEEDBACK_HTTP_TIMEOUT_MIN/MAX` (default 5..=300)
  - `ARW_FEEDBACK_MEM_LIMIT_MIN/MAX` (default 50..=2000)
- Effective policy: `GET /admin/feedback/policy` returns the current values.
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
- `GET /admin/feedback/state` → feedback state (signals, suggestions, auto_apply, delta log)
- `POST /admin/feedback/signal` → record a signal `{ kind, target, confidence, severity, note }`
- `POST /admin/feedback/analyze` → recompute suggestions immediately
- `POST /admin/feedback/apply` → `{ ok }` (policy‑gated; emits intents/events)
- `POST /admin/feedback/auto` → toggle auto-apply
- `POST /admin/feedback/reset` → clear signals & suggestions
- `GET /admin/feedback/suggestions` → `{ version, suggestions }`
- `GET /admin/feedback/updates?since=N` → 204 when unchanged
- `GET /admin/feedback/policy` → effective caps/bounds
- `GET /admin/feedback/versions` → available snapshots
- `POST /admin/feedback/rollback?to=N` → restore a previous snapshot (omit `to` for `.bak`)

## Specs
- OpenAPI: `/spec/openapi.yaml`
- AsyncAPI (events): `/spec/asyncapi.yaml` (includes feedback.* channels)
- MCP tools catalog: `/spec/mcp-tools.json`

## UI (Debug)
- Near‑live list with confidence badges and Apply buttons.
- Policy bounds/caps displayed inline; toasts on success/failure.
- Delta drawer (or CLI) shows `feedback.delta` entries (added / removed / changed suggestions) so operators can audit shadow runs before enabling auto-apply.

## Reviewing deltas (shadow mode)
- Tail `feedback.delta` via `arw-cli events tail --kind feedback.delta` (or your SSE client) during rehearsal; each payload includes `added`, `removed`, and `changed` arrays with summaries.
- `GET /admin/feedback/state` → `delta_log` retains the last 50 deltas so operators can cross-check after the fact. `arw-cli feedback state --json | jq '.delta_log[0]'` prints the most recent entry.
- Before flipping on auto-apply, confirm the last delta matches the sidecar approvals decisions and capture a quick note in the trial daily log.

Example `delta_log` entry:
```json
{
  "version": 12,
  "generated": "2025-10-03T11:22:33.456Z",
  "added": [
    {
      "id": "hint-http-timeout",
      "action": "hint",
      "params": { "route": "POST /actions" },
      "rationale": "Latency spike above 1.5s",
      "confidence": 0.82
    }
  ],
  "removed": [],
  "changed": [
    {
      "id": "mem-ring",
      "action": "mem_limit",
      "before": {
        "id": "mem-ring",
        "action": "mem_limit",
        "params": { "mb": 512 },
        "rationale": null,
        "confidence": 0.64
      },
      "after": {
        "id": "mem-ring",
        "action": "mem_limit",
        "params": { "mb": 640 },
        "rationale": "Sustained spillover observed",
        "confidence": 0.7
      }
    }
  ]
}
```
`changed` entries capture both the prior and updated suggestion payloads so reviewers can audit how the engine evolved a recommendation without relying on external state.

## Notes
- Keep `ARW_DEBUG=1` for local development; secure admin endpoints with `ARW_ADMIN_TOKEN` otherwise.
- For heavy loads, the engine drops/samples events rather than blocking; consumers can resync via `GET /admin/feedback/suggestions`.
