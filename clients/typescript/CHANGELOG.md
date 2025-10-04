## 0.2.4 - 2025-09-30

- Added `client.state.watchObservations/Beliefs/Intents/Actions` helpers that hydrate via SSE read-model patches (with automatic initial snapshots) so UIs can drop polling without custom plumbing.
- Added `state.beliefs()` / `state.intents()` snapshots and expanded `state.observations()` to accept `since` filters for parity with the CLI.
- Added `inactivityTimeoutMs` (Node fallback) to force reconnects when streams go silent, preventing ghost connections and aiding long-lived operators; CLI and examples expose the knob via `--idle`, `--structured`, `--out`, `--out-format`, `--out-max-bytes`, `--out-keep`, `--out-compress`, and sample settings.
- Browser `EventSource` path now shares lifecycle hooks while remaining non-destructive on idle (documented as Node-only enforcement) to avoid accidental hard closes.
- Docs/examples refreshed to demonstrate idle guards alongside existing reconnect tuning options.
- Added `examples/reliable_stream.ts` as a ready-made resilient follower with JSONL logging for operational scripts.

## 0.2.3 - 2025-09-30

- SSE clients now surface lifecycle changes across both browser `EventSource` and Node fallbacks, honour `retry:` hints, and expose `autoReconnect`/delay/jitter knobs via `EventsOptions` (propagated through `subscribeReadModel()` and `subscribePatches()`).
- Node fallback resumes with `Last-Event-ID` automatically and applies exponential backoff with jitter; disabling reconnects cleanly closes the stream.
- `arw-events` CLI gained `--no-reconnect`, `--delay`, `--max-delay`, and `--jitter` flags plus stderr status logs so operators can wire health indicators; examples now demonstrate status callbacks.

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
