## 0.2.2 - 2025-09-30

- Add `events.subscribeReadModel(id, opts)` helper that applies JSON Patch deltas, queues updates until an initial snapshot is available, and exposes `getSnapshot()`/`lastEventId()`. `loadInitial()` can hydrate via `GET /state/*`. Example `projects_patches.ts` now demonstrates it.
- Add `events.stream(opts)` for Node runtimes: yields parsed SSE payloads as an async generator with optional JSON parsing and `AbortSignal` support. CLI `arw-events` now uses it and keeps last-event id persistence.

## 0.2.1 - 2025-09-29

- Package metadata polish: add repository/homepage/bugs/keywords for better npm discoverability and docs linking. No API changes.

## 0.2.0 - 2025-09-29

- Add Node-friendly SSE implementation (no polyfills). Supports Last-Event-ID header, `replay`, and prefix filters.
- Add `subscribePatches(lastId?)` helper for `state.read.model.patch` streams.
- Add `arw-events` CLI (bin) to tail events with resume and filters.
- Packaging: `sideEffects: false`, Node >=18 engines; include DOM lib.
- Docs: examples for projects patches follower and events tail.

## 0.1.0 - 2025-09-29

- Initial minimal client for `/actions`, `/events`, `/healthz`, `/about`.
