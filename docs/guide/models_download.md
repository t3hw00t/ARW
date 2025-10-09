---
title: Models Download (HTTP)
---

# Models Download (HTTP)

ARW provides HTTP endpoints (admin‑gated) to manage local models with streaming downloads, live progress via SSE, safe cancel, and mandatory SHA‑256 verification. HTTP Range resume is supported when the upstream advertises validators (`ETag` or `Last-Modified`).

Updated: 2025-10-09
Type: How‑to

See also: Guide → Performance & Reasoning Playbook (budgets/admission), Reference → Configuration (ARW_DL_*, ARW_MODELS_*), and Architecture → Managed llama.cpp Runtime for how downloaded weights plug into the runtime supervisor.
Canonical topics used by the service are defined once under [crates/arw-topics/src/lib.rs](https://github.com/t3hw00t/ARW/blob/main/crates/arw-topics/src/lib.rs).

## Endpoints

- POST `/admin/models/download` — Start a download (requires `sha256`).
- POST `/admin/models/download/cancel` — Cancel an in‑flight download.
- GET  `/events` — Listen for `models.download.progress` events (SSE; supports `?replay=N` and repeated `prefix=` filters).
- GET  `/state/models` — Public, read‑only models list (no admin token required).
- GET  `/admin/models/summary` — Aggregated summary for UIs: `{ items, default, concurrency, metrics }`.
- POST `/admin/models/cas_gc` — Run a one‑off CAS GC sweep; deletes unreferenced blobs older than `ttl_hours` (default `24`). Set `"verbose": true` to include per-blob deletion details in the response payload.
- GET  `/state/models_hashes` — Summary of installed model hashes, sizes, providers, and referencing model IDs (`models`). Supports `provider=` and `model=` filters plus sorting for quick triage. Responses now include stable pagination metadata (`prev_offset`, `next_offset`, `page`, `pages`, `last_offset`) so UIs can jump directly to the first/previous/next/last slices without re-deriving offsets.
- GET  `/admin/models/by-hash/:sha256` — Serve a CAS blob by hash (egress‑gated; `io:egress:models.peer`). Responses include strong validators (`ETag:"{sha256}"`, `Last-Modified`) and long-lived caching headers so repeat fetches can short-circuit with `304 Not Modified` when unchanged. HEAD requests mirror the metadata without streaming the blob.
  - Emits strong validators and immutable caching for digest‑addressed blobs:
    - `ETag: "<sha256>"`, `Last-Modified`, `Cache-Control: public, max-age=31536000, immutable`.
    - Honors `If-None-Match` (304 Not Modified) for repeat requests.
  - See also: [HTTP Caching Semantics](../snippets/http_caching_semantics.md)
- POST `/admin/models/concurrency` — Set download concurrency at runtime. Body: `{ max?: number, hard_cap?: number, block?: boolean }`. When `block` is `true` (default), the call waits until active downloads fall under the new limit; set `block:false` to return immediately and monitor `pending_shrink` instead.
- GET  `/admin/models/concurrency` — Get the current concurrency settings and limits (`configured_max`, `available_permits`, `held_permits`, `hard_cap`, `pending_shrink`).
- GET  `/admin/models/jobs` — Snapshot of active jobs and inflight hashes for troubleshooting.

## Request

POST /admin/models/download

Body:

```
{
  "id": "<model-id>",
  "url": "https://.../model.gguf",
  "provider": "local",            // optional
  "sha256": "<hex>"               // required (fail-closed)
}
```

Behavior:
- Creates a temporary file `{state_dir}/models/<name>.part` and appends chunks.
- On completion and checksum verification, promotes into the content‑addressable store under `{state_dir}/models/by-hash/<sha256>[.<ext>]` and writes a manifest `{state_dir}/models/<id>.json` describing the model (`file`, `path`, `sha256`, `bytes`, `provider`, `verified`).
- If a model with the same content hash already exists in CAS, the download short‑circuits and finishes immediately with `code: "cached"`.
- Verifies the file against `sha256` and removes it on mismatch.
- Resume support (HTTP Range + If-Range) will return in a follow-up port; currently downloads always start from byte zero.

Filename handling:
- Cross‑platform sanitization avoids path separators, control/reserved characters, trailing dots/spaces, and Windows reserved device names (`CON`, `AUX`, `PRN`, `NUL`, `COM1..9`, `LPT1..9`).
- Length is capped (~120 chars), preserving the extension when possible.

## Cancel

POST /admin/models/download/cancel

```
{ "id": "<model-id>" }
```

Cancels the active download and removes the partial `.part` file.

Events related to cancel:
- `status: "canceled"` when the worker exits,
- if there is no active job, `status: "no-active-job"` is emitted.

## Progress (SSE)

Subscribe to `GET /events` and filter `models.download.progress` events. Examples:

```
{ "id": "qwen2.5-coder-7b", "status": "started", "url": "https://example/model.gguf" }
{ "id": "qwen2.5-coder-7b", "status": "downloading", "bytes": 26214400, "downloaded": 26214400, "total": 5347737600, "percent": 0.49 }
{ "id": "qwen2.5-coder-7b", "status": "resumed", "offset": 104857600 }
{ "id": "qwen2.5-coder-7b", "status": "complete", "sha256": "…", "bytes": 5347737600, "downloaded": 5347737600, "cached": false }
{ "id": "qwen2.5-coder-7b", "status": "canceled" }
{ "id": "qwen2.5-coder-7b", "status": "no-active-job" }
{ "id": "qwen2.5-coder-7b", "status": "error", "code": "sha256_mismatch", "error": "expected …" }

Schema notes:
- Always includes `id`.
- `status` is one of `started`, `queued`, `admitted`, `downloading`, `resumed`, `degraded`, `complete`, `canceled`, `cancel-requested`, `no-active-job`, or `error`.
- `code` provides a machine hint on progress/failure (e.g., `resumed`, `soft-budget`, `idle-timeout`, `sha256_mismatch`, `http`, `io`, `size_limit`, `quota_exceeded`, `disk_insufficient`).
- `bytes`/`downloaded` report cumulative bytes fetched; `total` and `percent` are present when the server provided `Content-Length` (`percent` is in the 0–100 range).
- `disk` surfaces download-time storage telemetry when `ARW_DL_PROGRESS_INCLUDE_DISK=1`: `{ available, reserve, need? }` (bytes). Use it to detect low space before replays retry.
- Completion events include `sha256`, `bytes`, `downloaded`, `cached`, and `total`.
- Every payload includes `corr_id`; use it to join egress ledger entries and `/events` flows.
```

Minimal SSE consumer (bash)
```bash
BASE=http://127.0.0.1:8091
curl -N -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" "$BASE/events?prefix=models.download.progress&replay=5" \
 | jq -rc 'if .id then {id:.id,status:(.status//""),code:(.code//""),bytes:(.bytes//null),cached:(.cached//null)} else . end'
```

Tip: use repeated `prefix=` to follow multiple channels: `...&prefix=models.download.progress&prefix=models.cas.gc`. `Last-Event-ID` resumes from the last journal row (equivalent to `?after=`).

### Resume behaviour
- Partial downloads are stored as `state/models/tmp/<hash>.part` with validators in `<hash>.part.meta` (ETag/Last-Modified). Removing either file forces a clean restart.
- On restart the worker verifies `Content-Range` matches the saved offset; mismatches produce `code:"resume-content-range"` and clean up the partial.
- `resumed` progress events include the prior offset; subsequent `downloading` events continue from that byte position.
- The ledger entry for the final decision includes the union of cached bytes and any resumed progress (`bytes_in`).
- `cancel-requested` signals when `POST /admin/models/download/cancel` is accepted; a follow-up `canceled` arrives when the worker exits.

## Egress Events

When downloads are offloaded, ARW emits compact egress events for observability:

- Preview before offload
```
{ "id": "qwen2.5-coder-7b", "url": "https://example/model.gguf", "dest": { "host": "example", "port": 443, "protocol": "https" }, "provider": "local", "corr_id": "..." }
```

- Ledger append (decision): allow/deny with attribution
```
{ "decision": "allow", "reason_code": "models.download", "posture": "off", "project_id": "default", "episode_id": null, "corr_id": "...", "node_id": null, "tool_id": "models.download", "dest": { "host": "example", "port": 443, "protocol": "https" }, "bytes_out": 0, "bytes_in": 1048576, "duration_ms": 1200 }
```

See also: Developer → [Egress Ledger Helper (Builder)](../developer/style.md#egress-ledger-helper-builder)

Note: formal `egress.decision` remains planned; previews and ledger appends are emitted today.

## Metrics

The downloader maintains a lightweight throughput EWMA used for admission checks.
- File: `{state_dir}/downloads.metrics.json` → `{ ewma_mbps }`
- State endpoint: `GET /state/models_metrics` → `{ ewma_mbps, …counters, runtime }`
- `runtime` reports idle timeout and retry tuning: `{ idle_timeout_secs, send_retries, stream_retries, retry_backoff_ms, preflight_enabled }`.
- Read‑model: `GET /state/models_metrics` (mirrors counters + EWMA) and SSE patches with id `models_metrics`.
 - SSE patches: `state.read.model.patch` with id=`models_metrics` publishes RFC‑6902 JSON Patches. Publishing is coalesced (`ARW_MODELS_METRICS_COALESCE_MS`, default 250ms) with an idle refresh (`ARW_MODELS_METRICS_PUBLISH_MS`, default 2000ms).

## Examples

Start a download (with checksum):

```bash
BASE=http://127.0.0.1:8091
curl -sS -X POST "$BASE/admin/models/download" \
  -H 'Content-Type: application/json' \
  -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -d '{"id":"qwen2.5-coder-7b","url":"https://example.com/qwen.gguf","sha256":"<hex>"}'
```

Cancel:

```bash
curl -sS -X POST "$BASE/admin/models/download/cancel" \
  -H 'Content-Type: application/json' \
  -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -d '{"id":"qwen2.5-coder-7b"}'
```

## Notes
- On failure, the model list is updated to `status: "error"` with `error_code` to avoid stuck "downloading" states.
- State directory is shown in `GET /admin/probe`.
- Concurrency: set `ARW_MODELS_MAX_CONC` (default 2) or `ARW_MODELS_MAX_CONC_HARD` to limit simultaneous downloads. When all permits are taken the caller waits for a free slot.
- Metrics: counters (`started`, `queued`, `admitted`, `canceled`, `completed`, `completed_cached`, `errors`, `bytes_total`) and throughput EWMA are exposed at `/state/models_metrics` and streamed via read‑model patches (`id: models_metrics`).
- Idle timeout defaults to `300` seconds (`ARW_DL_IDLE_TIMEOUT_SECS`). Set it to `0` to disable for very slow links or increase for high-latency mirrors.
- Admin UI (`/admin/ui/models`) surfaces runtime tuning (idle timeout, retry budgets, HEAD preflight state) and highlights common remediation steps when errors accumulate.
- Checksum: `sha256` is mandatory and must be a 64‑char hex string; invalid values are rejected up front.
- Budgets and disk-reserve enforcement now run in the unified server so long downloads surface `models.download.progress` events with optional `budget`/`disk` payloads (enable via `ARW_DL_PROGRESS_INCLUDE_*`).
- When elapsed time crosses `ARW_BUDGET_SOFT_DEGRADE_PCT` of the soft budget the server emits a one-time `status:"degraded"` progress event (`code:"soft-budget"`) before the soft limit is breached.
- When elapsed time reaches `ARW_BUDGET_DOWNLOAD_HARD_MS` the server cancels the transfer and emits an error progress event with `code:"hard-budget"`.
- Range resume is supported: ensure upstream responses provide `ETag` or `Last-Modified` so retries can be validated. The server retries automatically when the peer honours `If-Range` and within the configured retry budgets.

### Manifest

On success, a per‑ID manifest is written at `{state_dir}/models/<id>.json` describing the model and its CAS location. Schema: [spec/schemas/model_manifest.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/model_manifest.json).

### Policy & Egress

- The admin endpoint is gated by capability and egress policy: the request must satisfy `io:egress:models.download`.
- Optional egress ledger entries are appended for download attempts (success/failure); see “Egress & Provenance” docs. A compact `egress.preview` is emitted before offload with `{dest:{host,port,protocol}}` and `corr_id` for observability.

Security note: all `/admin/*` endpoints require either debug mode (`ARW_DEBUG=1`) or an admin token. Set `ARW_ADMIN_TOKEN` on the service and send it as `Authorization: Bearer <token>` or `X-ARW-Admin: <token>`.
GC unused blobs:

```bash
curl -sS -X POST "$BASE/admin/models/cas_gc" \
  -H 'Content-Type: application/json' \
  -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
 -d '{"ttl_hours":24,"verbose":true}'
```

GC emits a compact `models.cas.gc` event with `{scanned, kept, deleted, deleted_bytes, ttl_hours}`. When `verbose` is enabled, the HTTP response also includes a `deleted_items` array that lists each removed blob (`sha256`, `path`, `bytes`, `last_modified` when available).

Note: kinds are normalized; legacy CamelCase forms have been removed.
Get a summary suitable for dashboards:

```bash
curl -sS "$BASE/admin/models/summary" \
  -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" | jq .
```

Example response (wrapped in `{ ok, data }`):

```
{
  "ok": true,
  "data": {
    "items": [
      {"id":"llama3:8b","provider":"ollama","bytes":5347737600,"status":"ready"},
      {"id":"qwen2:7b","provider":"hf","bytes":4855592960,"status":"downloading"}
    ],
    "default": "llama3:8b",
    "concurrency": {"configured_max": 2, "available_permits": 2, "held_permits": 0},
    "metrics": {"started": 4, "queued": 1, "admitted": 3, "completed": 2, "bytes_total": 10245591040, "ewma_mbps": 18.2}
  }
}
```

Concurrency notes
- `max` adjusts the soft target; `hard_cap` enforces an upper bound even if `max` is higher.
