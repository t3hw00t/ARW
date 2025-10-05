---
title: Subscribe to Events (SSE)
---

# Subscribe to Events (SSE)
Updated: 2025-09-21
Type: How‑to

Microsummary: Connect to the live Server‑Sent Events stream, filter by prefix, and replay recent events. Admin‑gated. Stable.

Overview
- Endpoint (unified server): `GET /events` (text/event-stream)
- Base URL: `http://127.0.0.1:8091` for local development.
- Auth: requires admin access; set `ARW_ADMIN_TOKEN` and send `Authorization: Bearer <token>` if configured. Unauthorized requests return RFC 7807 ProblemDetails (title `Unauthorized`, status `401`).
- Filters: `?prefix=models.` (or any event kind prefix, e.g., `rpu.` for RPU trust events)
- Replay: `?replay=N` to emit the last N events on connect (best-effort)
- Resume: `?after=<row_id>` (or `Last-Event-ID`) to replay events after a given journal id (unified server)
- Correlation: producers set `payload.corr_id` whenever a job/request ID exists (e.g., downloads, egress ledger, actions) so downstream systems can stitch streams together.

Envelope
- Default mode: Each SSE message `data:` is a JSON envelope with at least `time`, `kind`, and `payload`. CloudEvents metadata (`ce`) is omitted unless the producer enriches the envelope:
  ```json
  {
    "time": "2025-01-01T00:00:00Z",
    "kind": "models.changed",
    "payload": { /* event-specific */ }
  }
  ```

- CloudEvents structured mode (opt-in): set `ARW_EVENTS_SSE_MODE=ce-structured` to emit CloudEvents 1.0 structured JSON in `data:` with `data` holding the event payload. Example (implementation in [apps/arw-server/src/api/events.rs](https://github.com/t3hw00t/ARW/blob/main/apps/arw-server/src/api/events.rs)):
  ```json
  {
    "specversion": "1.0",
    "type": "models.changed",
    "source": "urn:arw:server",
    "id": "2025-01-01T00:00:00Z",
    "time": "2025-01-01T00:00:00Z",
    "datacontenttype": "application/json",
    "data": { /* event-specific */ }
  }
  ```

- Resume & replay
- Unified server supports `?after=<row_id>` to replay after a specific journal id; also honors `Last-Event-ID` as an alias for `after` when present. SSE `id:` is set for replayed rows and best-effort for live events (mapped from the journal), enabling clients to resume using numeric row ids.
- Handshake: every connection starts with a `service.connected` envelope containing the generated `request_id`, the `resume_from` offset (when `Last-Event-ID` or `?after=` is supplied), the requested replay mode/count, and any `prefixes` so clients can log what was negotiated.
- The stream honors `Last-Event-ID` and supports `?replay=N`.

Examples
- Unified server (basic subscription with replay):
  ```bash
  curl -N \
    -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
    "http://127.0.0.1:8091/events?replay=20"
  ```
- Unified server with prefix filter and resume:
  ```bash
  curl -N \
    -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
    "http://127.0.0.1:8091/events?prefix=models.&after=12345"
  ```
- Unified server streaming only RPU trust events:
  ```bash
  curl -N \
    -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
    "http://127.0.0.1:8091/events?prefix=rpu.&replay=5"
  ```

CLI
- After publishing the TypeScript client, you can tail events via the bundled CLI:
  ```bash
  # Install globally (or use npx once published)
  npm i -g @arw/client

  # Tail service.* and read-model patches with replay and resume
  BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=$ARW_ADMIN_TOKEN \
  arw-events --prefix service.,state.read.model.patch --replay 25 --store .arw/last-event-id
  ```

Event model
- Events use a compact envelope with `status` (human) and `code` (machine) conventions.
- Common kinds: `models.download.progress`, `egress.ledger.appended`, `task.completed`, `feedback.suggested`, `feedback.delta`.
  - `screenshots.captured` includes `{ path, width, height, preview_b64? }` for Activity lane/Gallery thumbnails.
  - `egress.ledger.appended` includes `{ id?, decision, reason?, dest_host?, dest_port?, protocol?, bytes_in?, bytes_out?, corr_id?, proj?, posture }`.
 - RPU trust change: `rpu.trust.changed` (payload includes `{count, path?, ts_ms}`)
  - Capsule lifecycle: `policy.capsule.applied`, `policy.capsule.expired`, `policy.capsule.failed`, and `policy.capsule.teardown` (includes `{id, version, issuer?, removed_ms?, removed_reason?}`).

Note: event kinds are normalized. Legacy `Models.*` forms have been removed.
- See Explanations → Events Vocabulary for the canonical list. For source‑of‑truth topic names used by the service, see `crates/arw-topics/src/lib.rs`.

Context assembly emits structured telemetry on the same bus:
- `context.coverage` — carries `needs_more`, `reasons`, and the full summary/spec snapshots including `slots.budgets` and `slots.counts` so you can highlight under-filled budgets without rehydrating the request.
- `context.recall.risk` — exposes the blended `score`, `level`, `components.slots`, and the spec snapshot (`slot_budgets`, `query_provided`, `project`) for dashboards that surface recall regressions.
  ```bash
  curl -N \
    -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
    "http://127.0.0.1:8091/events?prefix=context.coverage&prefix=context.recall.risk&replay=5" \
  | jq '.kind as $k | if $k == "context.coverage" then {kind:$k,needs_more:.needs_more,reasons:.reasons,slot_counts:.summary.slots.counts,slot_budgets:.summary.slots.budgets} else {kind:$k,level:.level,components:.components,slot_budgets:.spec.slot_budgets} end'
  ```

Tips
- Stitch episodes using `corr_id` on each event.
- Use `?prefix=` to scope dashboards without client-side filtering cost.
- For production, proxy and secure `/events` endpoints behind admin access; do not expose publicly.
