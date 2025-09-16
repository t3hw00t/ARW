---
title: Events Vocabulary
---

# Events Vocabulary
Updated: 2025-09-16
Type: Explanation

Normalize ARW events into a small internal vocabulary. Drive all live UI from this stream via `GET /admin/events` (SSE; admin‑gated). Use `corr_id` to stitch episodes.

Reference
- Canonical topic names are centralized as constants in `crates/arw-topics/src/lib.rs` and used throughout the service and unified server for consistency.

Canonical categories (normalized)
- Episode lifecycle: `episode.started`, `episode.completed`, `episode.canceled`, `episode.error`
- Thought stages: `obs.*` (observations), `beliefs.*`, `intents.*`, `actions.*`
- Token I/O: `tokens.in`, `tokens.out`
- Tooling: `tool.invoked`, `tool.ran`, `tool.error`
- Context: `working_set.started`, `working_set.seed`, `working_set.expanded`, `working_set.expand_query`, `working_set.selected`, `working_set.iteration.summary`, `working_set.completed`, `working_set.error` (payload includes `iteration`, `project`, `query`, and optional `corr_id`; summaries also include the iteration's spec snapshot, a `coverage{needs_more,reasons}` object, and—when another pass is queued—a `next_spec` snapshot. Errors echo the spec alongside the message)
- Policy: `policy.prompt`, `policy.allow`, `policy.deny`
- Runtime: `runtime.health`, `runtime.profile.changed`
- Models: `models.download.progress`, `models.changed`, `models.cas.gc`, `models.manifest.written`, `models.refreshed`
 - Interactive performance (snappy): read‑model id=`snappy` via `state.read.model.patch`; events `snappy.notice` (breach), `snappy.detail` (periodic detail).
  - models.download.progress statuses may include: `started`, `queued`, `admitted`, `resumed`, `downloading`, `resync`, `degraded` (soft budget), `cancel-requested`, `complete`, `error`, `canceled`, `no-active-job`, `cache-mismatch`.
- Self‑Model: `self.model.proposed`, `self.model.updated`
- Logic Units: `logic.unit.suggested`, `logic.unit.installed`, `logic.unit.applied`, `logic.unit.reverted`, `logic.unit.promoted`
- Cluster: `cluster.node.advertise`, `cluster.node.heartbeat`, `cluster.node.changed`
- Jobs (offload): `job.assigned`, `job.progress`, `job.completed`, `job.error`
- Sessions (sharing): `session.invited`, `session.role.changed`, `session.event.relayed`
- Egress: `egress.preview` (pre‑offload summary), `egress.ledger.appended` (append‑only record)
  - `egress.decision` remains planned; today we emit previews and ledger appends for downloads and select offloads.
  - See also: Developer → [Egress Ledger Helper (Builder)](../developer/style.md#egress-ledger-helper-builder)
- Memory (planned): `memory.quarantined`, `memory.admitted`
- World diffs (planned): `world.diff.queued`, `world.diff.conflict`, `world.diff.applied`
- Cluster trust (planned): `cluster.manifest.published`, `cluster.manifest.trusted`, `cluster.manifest.rejected`, `cluster.event.rejected`
- Archives (planned): `archive.unpacked`, `archive.blocked`
- DNS (planned): `dns.anomaly`

Minimal event envelope
```
{ time, kind, code, status, id?, corr_id?, span_id?, payload, severity? }
```

Notes
- `status` is human‑friendly; `code` is a stable machine hint (e.g., `admission-denied`, `hard-exhausted`).
- Vocabulary is small on purpose; keep renderers simple and deterministic.

Mapping from existing ARW events
- Observations/Beliefs/Intents/Actions are already exposed under `/state/*` and emitted in debug builds; clients can mirror or subscribe.
- `models.download.progress` supports `{ id, status|error, code, budget?, disk? }` — see `apps/arw-svc/src/resources/models_service.rs`.
  - Common `code` values: `admission-denied`, `hard-exhausted`, `disk-insufficient(-stream)`, `size-limit(-stream)`, `checksum-mismatch`, `cache-mismatch`, `canceled-by-user`, `already-in-progress-hash`, `quota-exceeded`, `cached`, `resync`.
- Download start is represented via `models.download.progress` with `status:"started"`.
- `models.manifest.written` is emitted after a successful write of `<state>/models/<id>.json`.
- `models.cas.gc` emits `{scanned, kept, deleted, deleted_bytes, ttl_days}` after a GC sweep.
- `models.changed` publishes ops like `add`, `delete`, `default`, `downloaded`, `canceled`, `error`.
- `models.refreshed` publishes a count after resetting the models list to defaults.
- Cluster events are additive and off by default. When enabled, Workers publish `cluster.node.advertise` (capabilities, health), periodic `cluster.node.heartbeat`, and receive `job.*` assignments. The Home Node merges remote `job.*` and `session.*` events into the unified timeline by `corr_id`.
- World model (read‑model) materializes from existing events like `feedback.suggested` / `beliefs.updated`, `projects.file.written`, `actions.hint.applied`, `runtime.health`, and `models.download.progress`. A compact `world.updated` event is emitted with counts and version for UI/SSE.
- Egress firewall emits previews and ledger appends today. An append‑only ledger records normalized entries with episode/project/node attribution. Decisions remain planned.
 - Model downloads emit progress heartbeats and budget/disk hints; when enabled, egress ledger entries are appended for `allow` and `deny` decisions around downloads.
 - Memory quarantine emits planned events; a compact review queue materializes under `/state/memory/quarantine`.
 - Cluster trust uses planned manifest events; scheduler logs pin/deny reasons as codes.
- Tools registered via `#[arw_tool]` already emit `tool.ran` with inputs/outputs summary.
- Read‑models: small server‑maintained summaries publish RFC‑6902 JSON Patch deltas via `state.read.model.patch` with coalescing:
   - id=`models_metrics` — models counters + EWMA.
   - id=`route_stats` — route latencies/hits/errors.
  Clients can also fetch `/state/models_metrics` and `/state/route_stats` for full snapshots.

## Topics Summary (quick reference)

| Kind                       | Purpose                          | Payload key points |
|----------------------------|----------------------------------|--------------------|
| models.download.progress   | Download lifecycle and errors    | id, status/error, code, budget?, disk?, progress?, downloaded, total? |
| models.changed             | Models list deltas               | op (add/delete/default/downloaded/canceled/error), id, path? |
| models.refreshed           | Default models list refreshed    | count |
| models.manifest.written    | Per‑ID manifest written          | id, manifest_path, sha256 |
| models.cas.gc              | CAS GC sweep summary             | scanned, kept, deleted, deleted_bytes, ttl_days |
| egress.preview             | Pre‑offload destination summary  | id, url (redacted), dest{host,port,protocol}, provider, corr_id |
| egress.ledger.appended     | Egress ledger entry appended     | id?, decision, reason?, dest(host,port,protocol), bytes_in/out, corr_id?, proj?, posture |
| state.read.model.patch     | Read‑model JSON Patch deltas     | id, patch[...] |
| snappy.notice              | Snappy budgets: breach notice    | p95_max_ms, budget_ms |
| snappy.detail              | Snappy budgets: periodic detail  | p95_by_path{"/path":p95_ms} |
- Self‑Model endpoints emit compact events: `self.model.proposed` (agent, proposal_id, rationale, widens_scope?) and `self.model.updated` (agent, proposal_id). Read‑models available at `/state/self` and `/state/self/{agent}`.

Replay and filtering
- SSE supports `?replay=N` and lightweight prefix filters (`?prefix=models.`) for scoped dashboards.

Migration
- The service publishes normalized kinds (dot.case) only. Legacy `Models.*` event names have been removed.

UI routing
- The universal sidecar subscribes once and filters by `corr_id`/scope to render a live, consistent timeline.
