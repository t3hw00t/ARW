---
title: Subscribe to Events (SSE)
---

# Subscribe to Events (SSE)
Updated: 2025-09-16
Type: How‑to

Microsummary: Connect to the live Server‑Sent Events stream, filter by prefix, and replay recent events. Admin‑gated. Stable.

Overview
- Endpoint (unified server): `GET /events` (text/event-stream)
- Endpoint (legacy svc): `GET /admin/events` (text/event-stream)
- Auth: requires admin access; set `ARW_ADMIN_TOKEN` and use it as `Authorization: Bearer <token>` if configured.
- Filters: `?prefix=models.` (or any event kind prefix, e.g., `rpu.` for RPU trust events)
- Replay: `?replay=N` to emit the last N events on connect (best‑effort)
- Resume: `?after=<row_id>` to replay events after a given journal id (unified server)

Envelope
- Default mode: Each SSE message `data:` is a JSON envelope with at least `time`, `kind`, and `payload`. CloudEvents 1.0 metadata is included under `ce`:
  ```json
  {
    "time": "2025-01-01T00:00:00Z",
    "kind": "models.changed",
    "payload": { /* event-specific */ },
    "ce": {
      "specversion": "1.0",
      "type": "models.changed",
      "source": "arw-svc",
      "id": "2025-01-01T00:00:00Z",
      "time": "2025-01-01T00:00:00Z",
      "datacontenttype": "application/json"
    }
  }
  ```

- CloudEvents structured mode (opt‑in): set `ARW_EVENTS_SSE_MODE=ce-structured` to emit CloudEvents 1.0 structured JSON in `data:` with `data` holding the event payload. Example:
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
- Unified server supports `?after=<row_id>` to replay after a specific journal id; also honors `Last-Event-ID` as an alias for `after` when present. SSE `id:` is set for replayed rows and best‑effort for live events (mapped from the journal), enabling clients to resume using numeric row ids.
- Legacy svc honors `Last-Event-ID` and supports `?replay=N`.

Examples
- Basic subscription (Unix):
  ```bash
  curl -N http://127.0.0.1:8090/admin/events
  ```
- With admin token and filter:
  ```bash
  curl -N \
    -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
    "http://127.0.0.1:8090/admin/events?prefix=models.&replay=10"
  ```
  Watch only RPU trust events:
  ```bash
  curl -N \
    -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
    "http://127.0.0.1:8090/admin/events?prefix=rpu.&replay=5"
  ```

Event model
- Events use a compact envelope with `status` (human) and `code` (machine) conventions.
- Common kinds: `models.download.progress`, `egress.ledger.appended`, `task.completed`, `feedback.suggested`.
  - `egress.ledger.appended` includes `{ id?, decision, reason?, dest_host?, dest_port?, protocol?, bytes_in?, bytes_out?, corr_id?, proj?, posture }`.
 - RPU trust change: `rpu.trust.changed` (payload includes `{count, path?, ts_ms}`)

Note: event kinds are normalized. Legacy `Models.*` forms have been removed.
- See Explanations → Events Vocabulary for the canonical list. For source‑of‑truth topic names used by the service, see `crates/arw-topics/src/lib.rs`.

Tips
- Stitch episodes using `corr_id` on each event.
- Use `?prefix=` to scope dashboards without client‑side filtering cost.
- For production, proxy and secure `/admin/*` endpoints; do not expose publicly.
