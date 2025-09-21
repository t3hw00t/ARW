# SSE + JSON Patch Contract
Updated: 2025-09-21
Type: Explanation

Purpose: stream deltas, not snapshots, to keep interaction latency low and bytes small. Resume connections without losing context.

- Transport: `text/event-stream` (SSE), HTTP/2 preferred.
- Event kinds: domain events (`models.download.progress`, `chat.message`), read‑model patches (`state.*.patch`), notices, and a startup ack. Canonical constants live in [crates/arw-topics/src/lib.rs](https://github.com/t3hw00t/ARW/blob/main/crates/arw-topics/src/lib.rs).
- Patch format: RFC 6902 (`application/json-patch+json`) with compact paths.
- Resume: client may send `Last-Event-ID`. Server acks with `service.connected` including `request_id`, `resume_from`, and replay metadata so clients can correlate logs. The query `?replay=N` replays from the in-process buffer (best-effort).

Example handshake:

1) Client connects with optional `Last-Event-ID: 2025-09-13T00:00:00.001Z`.
2) Server emits:

```
id: 0
event: service.connected
data: {"request_id":"2d0f6a82-5be8-4e5c-8a5d-2b799d8d9f9e","resume_from":"2025-09-13T00:00:00.001Z","replay":{"mode":"after","count":0},"prefixes":null,"kernel_replay":true}
```

3) Server then streams replay (if `?replay=N`) followed by live events.

Read‑model patch topics:

- `State.<Name>.Patch` — specific topic for a model. Payload: `{ id: "<id>", patch: <json-patch-array> }`.
- `state.read.model.patch` — generic topic. Payload: `{ id: "<id>", patch: <json-patch-array> }`. See `TOPIC_READMODEL_PATCH` in [crates/arw-topics/src/lib.rs](https://github.com/t3hw00t/ARW/blob/main/crates/arw-topics/src/lib.rs).

Clients can apply patches to a local object and render without re-fetching full snapshots.
