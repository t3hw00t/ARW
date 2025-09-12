---
title: Events Vocabulary
---

# Events Vocabulary

Normalize ARW events into a small internal vocabulary. Drive all live UI from this stream via `GET /events` (SSE). Use `corr_id` to stitch episodes.

Canonical categories
- Episode lifecycle: `Episode.Started`, `Episode.Completed`, `Episode.Canceled`, `Episode.Error`
- Thought stages: `Obs` (observations), `Bel` (beliefs), `Int` (intents), `Act` (actions)
- Token I/O: `Tokens.In`, `Tokens.Out`
- Tooling: `Tool.Invoked`, `Tool.Ran`, `Tool.Error`
- Policy: `Policy.Prompt`, `Policy.Allow`, `Policy.Deny`
- Runtime: `Runtime.Health`, `Runtime.ProfileChanged`
- Models: `Models.DownloadProgress`, `Models.Changed`
- Logic Units: `LogicUnit.Suggested`, `LogicUnit.Installed`, `LogicUnit.Applied`, `LogicUnit.Reverted`, `LogicUnit.Promoted`

Minimal event envelope
```
{ time, kind, code, status, id?, corr_id?, span_id?, payload, severity? }
```

Notes
- `status` is human‑friendly; `code` is a stable machine hint (e.g., `admission_denied`, `hard_exhausted`).
- Vocabulary is small on purpose; keep renderers simple and deterministic.

Mapping from existing ARW events
- Observations/Beliefs/Intents/Actions are already exposed under `/state/*` and emitted in debug builds; clients can mirror or subscribe.
- `Models.DownloadProgress` supports `{ id, status|error, code, budget?, disk? }` — see `resources/models_service.rs`.
- Tools registered via `#[arw_tool]` already emit `Tool.Ran` with inputs/outputs summary.

Replay and filtering
- SSE supports `?replay=N` and lightweight prefix filters (`?prefix=Models.`) for scoped dashboards.

UI routing
- The universal sidecar subscribes once and filters by `corr_id`/scope to render a live, consistent timeline.
