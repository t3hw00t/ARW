## 0.2.0 - 2025-09-29

- Add Node-friendly SSE implementation (no polyfills). Supports Last-Event-ID header, `replay`, and prefix filters.
- Add `subscribePatches(lastId?)` helper for `state.read.model.patch` streams.
- Add `arw-events` CLI (bin) to tail events with resume and filters.
- Packaging: `sideEffects: false`, Node >=18 engines; include DOM lib.
- Docs: examples for projects patches follower and events tail.

## 0.1.0 - 2025-09-29

- Initial minimal client for `/actions`, `/events`, `/healthz`, `/about`.

