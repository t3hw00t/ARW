# SSE + JSON Patch Contract

Purpose: stream deltas, not snapshots, to keep interaction latency low and bytes small. Resume connections without losing context.

- Transport: `text/event-stream` (SSE), HTTP/2 preferred.
- Event kinds: domain events (`Models.DownloadProgress`, `Chat.Message`), read‑model patches (`State.*.Patch`), notices, and a startup ack.
- Patch format: RFC 6902 (`application/json-patch+json`) with compact paths.
- Resume: client may send `Last-Event-ID`. Server acks with `Service.Connected` and includes `{"resume_from":"<id>"}` for clients to decide replay strategy. The query `?replay=N` replays from the in‑process buffer (best‑effort).

Example handshake:

1) Client connects with optional `Last-Event-ID: 2025-09-13T00:00:00.001Z`.
2) Server emits:

```
id: 0
event: Service.Connected
data: {"resume_from":"2025-09-13T00:00:00.001Z"}
```

3) Server then streams replay (if `?replay=N`) followed by live events.

Read‑model patch topics:

- `State.<Name>.Patch` — specific topic for a model. Payload: `{ id: "<id>", patch: <json-patch-array> }`.
- `State.ReadModel.Patch` — generic topic. Payload: `{ id: "<id>", patch: <json-patch-array> }`.

Clients can apply patches to a local object and render without re-fetching full snapshots.

