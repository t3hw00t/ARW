---
title: Events Vocabulary
---

# Events Vocabulary

Normalize ARW events into a small internal vocabulary. Drive all live UI from this stream via `GET /admin/events` (SSE; admin‑gated). Use `corr_id` to stitch episodes.

Canonical categories
- Episode lifecycle: `Episode.Started`, `Episode.Completed`, `Episode.Canceled`, `Episode.Error`
- Thought stages: `Obs` (observations), `Bel` (beliefs), `Int` (intents), `Act` (actions)
- Token I/O: `Tokens.In`, `Tokens.Out`
- Tooling: `Tool.Invoked`, `Tool.Ran`, `Tool.Error`
- Policy: `Policy.Prompt`, `Policy.Allow`, `Policy.Deny`
- Runtime: `Runtime.Health`, `Runtime.ProfileChanged`
- Models: `Models.DownloadProgress`, `Models.Changed`
- Self‑Model: `SelfModel.Proposed`, `SelfModel.Updated`
- Logic Units: `LogicUnit.Suggested`, `LogicUnit.Installed`, `LogicUnit.Applied`, `LogicUnit.Reverted`, `LogicUnit.Promoted`
 - Cluster: `Cluster.Node.Advertise`, `Cluster.Node.Heartbeat`, `Cluster.Node.Changed`
 - Jobs (offload): `Job.Assigned`, `Job.Progress`, `Job.Completed`, `Job.Error`
 - Sessions (sharing): `Session.Invited`, `Session.RoleChanged`, `Session.EventRelayed`
 - Egress (planned): `Egress.Decision` (allow/deny + reason), `Egress.Preview` (pre‑offload summary), `Egress.Ledger.Appended`
 - Memory (planned): `Memory.Quarantined`, `Memory.Admitted`
 - World diffs (planned): `WorldDiff.Queued`, `WorldDiff.Conflict`, `WorldDiff.Applied`
 - Cluster trust (planned): `Cluster.ManifestPublished`, `Cluster.ManifestTrusted`, `Cluster.ManifestRejected`, `Cluster.EventRejected`
 - Archives (planned): `Archive.Unpacked`, `Archive.Blocked`
 - DNS (planned): `Dns.Anomaly`

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
- Cluster events are additive and off by default. When enabled, Workers publish `Cluster.Node.Advertise` (capabilities, health), periodic `Cluster.Node.Heartbeat`, and receive `Job.*` assignments. The Home Node merges remote `Job.*` and `Session.*` events into the unified timeline by `corr_id`.
- World model (read‑model) materializes from existing events like `Feedback.Suggested` / `Beliefs.Updated`, `Projects.FileWritten`, `Actions.HintApplied`, `Runtime.Health`, and `Models.DownloadProgress`. A compact `World.Updated` event is emitted with counts and version for UI/SSE.
- Egress firewall emits planned events for previews and decisions; an append‑only ledger records normalized entries with episode/project/node attribution.
 - Memory quarantine emits planned events; a compact review queue materializes under `/state/memory/quarantine`.
 - Cluster trust uses planned manifest events; scheduler logs pin/deny reasons as codes.
- Tools registered via `#[arw_tool]` already emit `Tool.Ran` with inputs/outputs summary.
- Self‑Model endpoints emit compact events: `SelfModel.Proposed` (agent, proposal_id, rationale, widens_scope?) and `SelfModel.Updated` (agent, proposal_id). Read‑models available at `/state/self` and `/state/self/{agent}`.

Replay and filtering
- SSE supports `?replay=N` and lightweight prefix filters (`?prefix=Models.`) for scoped dashboards.

UI routing
- The universal sidecar subscribes once and filters by `corr_id`/scope to render a live, consistent timeline.
