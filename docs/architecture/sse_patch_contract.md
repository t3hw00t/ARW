# SSE + JSON Patch Contract
Updated: 2025-09-30
Type: Explanation

Purpose: stream deltas, not snapshots, to keep interaction latency low and bytes small. Resume connections without losing context.

- Transport: `text/event-stream` (SSE), HTTP/2 preferred.
- Event kinds: domain events (`models.download.progress`, `chat.message`), read‑model patches (`state.*.patch`), notices, and a startup ack. Canonical constants live in [crates/arw-topics/src/lib.rs](https://github.com/t3hw00t/ARW/blob/main/crates/arw-topics/src/lib.rs).
- Patch format: RFC 6902 (`application/json-patch+json`) with compact paths.
- Resume: client may send `Last-Event-ID`. Server acks with `service.connected` including `request_id`, `resume_from`, and replay metadata so clients can correlate logs. The query `?replay=N` replays from the in-process buffer (best-effort).
- IDs: when the kernel is enabled every event is persisted and receives a monotonic row id; the SSE stream reuses that id so clients can reconnect with `Last-Event-ID` and fetch missed JSON Patch deltas directly from `/events`.

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

- `state.read.model.patch` — canonical topic for all read-model deltas. Payload: `{ id: "<id>", patch: <json-patch-array> }`. See `TOPIC_READMODEL_PATCH` in [crates/arw-topics/src/lib.rs](https://github.com/t3hw00t/ARW/blob/main/crates/arw-topics/src/lib.rs).
- Per-model topics (e.g., `state.projects.patch`) were retired with the dot.case rollout. Consumers should filter by the payload `id` to target a specific read-model.

Clients can apply patches to a local object and render without re-fetching full snapshots.

## Hub Projects Walkthrough

The desktop Hub uses the contract above to keep the Projects panel in sync:

1. **Prime from REST** — on launch it fetches the cached read-model snapshot from `/state/projects` so the tree renders even before the SSE channel opens.
2. **Subscribe** — it connects to `/events?prefix=state.read.model.patch&replay=25` and records the `id` for every `state.read.model.patch` event.
3. **Apply patches** — when the payload contains `{ "id": "projects" }` the Hub merges the JSON Patch into its local store, updating notes metadata and directory listings without additional HTTP calls.
4. **Resume** — if the SSE socket reconnects, the Hub sends `Last-Event-ID` (the kernel row id). The server replies with `service.connected` and replays any missed patches, ensuring the UI’s caches match the persisted journal.

This pattern is reused for other read-model consumers (e.g., Snappy telemetry) so clients only pay the cost of one initial snapshot and then stream compact diffs.
