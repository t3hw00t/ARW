---
title: Models Download (HTTP)
---

# Models Download (HTTP)

ARW provides HTTP endpoints (admin‑gated) to manage local models with streaming downloads, live progress via SSE, safe cancel, resume (HTTP Range), and mandatory SHA‑256 verification.

Updated: 2025-09-12

See also: Guide → Performance & Reasoning Playbook (budgets/admission), Reference → Configuration (ARW_DL_*, ARW_MODELS_*).

## Endpoints

- POST `/admin/models/download` — Start or resume a download.
- POST `/admin/models/download/cancel` — Cancel an in‑flight download.
- GET  `/admin/events` — Listen for `Models.DownloadProgress` events (SSE; supports `?replay=N` and repeated `prefix=` filters).
- GET  `/state/models` — Public, read‑only models list (no admin token required).
- POST `/admin/models/cas_gc` — Run a one‑off CAS GC sweep; deletes unreferenced blobs older than `ttl_days`.
- GET  `/admin/state/models_hashes` — Admin summary of installed model hashes and sizes.
- GET  `/admin/models/by-hash/:sha256` — Serve a CAS blob by hash (egress‑gated; `io:egress:models.peer`).
  - Emits strong validators and immutable caching for digest‑addressed blobs:
    - `ETag: "<sha256>"`, `Last-Modified`, `Cache-Control: public, max-age=31536000, immutable`.
    - Honors `If-None-Match` (304 Not Modified) for repeat requests.

## Request

POST /admin/models/download

Body:

```
{
  "id": "<model-id>",
  "url": "https://.../model.gguf",
  "provider": "local",            // optional
  "sha256": "<hex>",             // required (fail-closed)
  "budget": {                      // optional override
    "soft_ms": 15000,
    "hard_ms": 60000,
    "class":   "interactive"      // or "batch"
  }
}
```

Behavior:
- Creates a temporary file `{state_dir}/models/<name>.part` and appends chunks.
- On completion and checksum verification, promotes into the content‑addressable store under `{state_dir}/models/by-hash/<sha256>[.<ext>]` and writes a manifest `{state_dir}/models/<id>.json` describing the model (`file`, `path`, `sha256`, `bytes`, `provider`, `verified`).
- If a model with the same content hash already exists in CAS, the download short‑circuits and finishes immediately with `code: "cached"`.
- Honors `Content-Disposition: attachment; filename=...` and `filename*=` (RFC 5987) to pick a server-provided filename; names are sanitized cross‑platform.
- Verifies the file against `sha256` and removes it on mismatch.
- If a `.part` exists and the server supports HTTP Range, ARW resumes from the saved offset.
  - Uses `If-Range` with previously observed `ETag`/`Last-Modified` to avoid corrupt resumes when the remote file changed.

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
- `status: "cancel-requested"` (immediate acknowledgment),
- `status: "canceled"` when the worker exits,
- if there is no active job, `status: "no-active-job"` is emitted.

## Progress (SSE)

Subscribe to `GET /admin/events` and filter `Models.DownloadProgress` events. Examples:

```
{ "id": "qwen2.5-coder-7b", "progress": 42, "downloaded": 12345678, "total": 30000000 }
{ "id": "qwen2.5-coder-7b", "status": "resumed", "offset": 102400 }
{ "id": "qwen2.5-coder-7b", "status": "complete", "file": "qwen.gguf", "provider": "local" }
{ "id": "qwen2.5-coder-7b", "error": "checksum mismatch", "expected": "...", "actual": "..." }
{ "id": "qwen2.5-coder-7b", "status": "canceled" }

Schema notes (best effort):
- Always includes: `id`.
- Progress: `progress` (0–100), `downloaded`, `total` (optional).
- Status: `status` (e.g., started, resumed, downloading, degraded, complete, canceled).
- Codes: `code` provides a stable machine hint for complex statuses (e.g., `admission_denied`, `hard_exhausted`, `disk_insufficient(_stream)`, `size_limit(_stream)`, `checksum_mismatch`, `canceled_by_user`, `quota_exceeded`, `cached`, `already-in-progress-hash`, `resync`).
- Budget snapshot: `budget` object with `soft_ms`, `hard_ms`, `spent_ms`, `remaining_*` when available.
- Disk snapshot: `disk` object `{available,total,reserve}` when available.

UI guidance:
- Simple statuses (started/downloading/resumed/complete/canceled) should use compact single icons.
- Complex codes can show a small, subtle icon set (e.g., `lock+timer` for `admission_denied`).
```

Minimal SSE consumer (bash)
```bash
BASE=http://127.0.0.1:8090
curl -N -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" "$BASE/admin/events?prefix=Models.DownloadProgress&replay=5" \
 | jq -rc 'if .id then {id:.id,status:(.status//""),code:(.code//""),pct:(.progress//null),dl:(.downloaded//null),tot:(.total//null)} else . end'
```

Tip: use repeated `prefix=` to follow multiple channels: `...&prefix=Models.DownloadProgress&prefix=Models.CasGc`.

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

Note: formal `Egress.Decision` remains planned; previews and ledger appends are emitted today.

## Metrics

The downloader maintains a lightweight throughput EWMA used for admission checks.
- File: `{state_dir}/downloads.metrics.json` → `{ ewma_mbps }`
- Admin endpoint: `GET /admin/models/downloads_metrics`
 - Read‑model: `GET /admin/state/models_metrics` and public `GET /state/models_metrics` (mirrors counters + EWMA).
 - SSE patches: `State.ModelsMetrics.Patch` and generic `State.ReadModel.Patch` (id=`models_metrics`) publish RFC‑6902 JSON Patches. Publishing is coalesced (`ARW_MODELS_METRICS_COALESCE_MS`, default 250ms) with an idle refresh (`ARW_MODELS_METRICS_PUBLISH_MS`, default 2000ms).

## Examples

Start a download (with checksum):

```bash
BASE=http://127.0.0.1:8090
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

Resume:
- Re-issue the same `POST /admin/models/download` request. If the server honors `Range: bytes=<offset>-`, ARW resumes from the existing `.part` file.

## Notes
- When `total` is unknown, events may omit it and include only `downloaded`.
- On failure, the model list is updated to `status: "error"` with `error_code` to avoid stuck "downloading" states.
- State directory is shown in `GET /admin/probe`.
- Concurrency: set `ARW_MODELS_MAX_CONC` (default 2) to limit simultaneous downloads. When saturated, a download emits `status: "queued"` and then `"admitted"` once it starts.
 - Live counters: `started, queued, admitted, resumed, canceled, completed, completed_cached, errors, bytes_total` are exported to Prometheus at `/metrics` as `arw_models_download_*` and to the read‑model at `/state/models_metrics`.
- Disk safety: the downloader reserves space to avoid filling the disk. Set `ARW_MODELS_DISK_RESERVE_MB` (default 256) to control the reserved free‑space buffer. If there isn’t enough free space for the download, it aborts with an error event.
- Size caps: set `ARW_MODELS_MAX_MB` (default 4096) to cap the maximum allowed size per download. The cap is enforced using the `Content-Length` when available and during streaming when it isn’t.
- Checksum: when `sha256` is provided, it must be a 64‑char hex string; invalid values are rejected up front.
- Progress payloads can include a budget snapshot (`budget`) and disk info (`disk`) when enabled via env:
  - `ARW_DL_PROGRESS_INCLUDE_BUDGET=1`
  - `ARW_DL_PROGRESS_INCLUDE_DISK=1`
  Related tuning knobs: `ARW_MODELS_MAX_MB`, `ARW_MODELS_DISK_RESERVE_MB`, `ARW_DL_MIN_MBPS`, `ARW_DL_EWMA_ALPHA`, `ARW_DL_SEND_RETRIES`, `ARW_DL_STREAM_RETRIES`, `ARW_DL_IDLE_TIMEOUT_SECS`, `ARW_BUDGET_SOFT_DEGRADE_PCT`.
 - Admission checks: when `total` is known, the downloader estimates if it can finish within the remaining hard budget using a throughput baseline `ARW_DL_MIN_MBPS` and a persisted EWMA. If not, it emits `code: "admission_denied"`.
 - Idle safety: when no hard budget is set, `ARW_DL_IDLE_TIMEOUT_SECS` applies an idle timeout to avoid hung transfers.
- Quota & preflight: when `ARW_DL_PREFLIGHT=1`, a HEAD request captures `Content-Length` and validators. If `ARW_MODELS_QUOTA_MB` is set, the preflight denies downloads whose projected CAS size would exceed the quota, emitting `code: "quota_exceeded"`. Early size checks also enforce `ARW_MODELS_MAX_MB`.

### Manifest

On success, a per‑ID manifest is written at `{state_dir}/models/<id>.json` describing the model and its CAS location. Schema: `spec/schemas/model_manifest.json`.

### Policy & Egress

- The admin endpoint is gated by capability and egress policy: the request must satisfy `io:egress:models.download`.
- Optional egress ledger entries are appended for download attempts (success/failure); see “Egress & Provenance” docs. A compact `Egress.Preview` is emitted before offload with `{dest:{host,port,protocol}}` and `corr_id` for observability.

Security note: all `/admin/*` endpoints require either debug mode (`ARW_DEBUG=1`) or an admin token. Set `ARW_ADMIN_TOKEN` on the service and send it as `Authorization: Bearer <token>` or `X-ARW-Admin: <token>`.
GC unused blobs:

```bash
curl -sS -X POST "$BASE/admin/models/cas_gc" \
  -H 'Content-Type: application/json' \
  -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -d '{"ttl_days":14}'
```

GC emits a compact `Models.CasGc` event with `{scanned, kept, deleted, deleted_bytes, ttl_days}`.
