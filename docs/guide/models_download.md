---
title: Models Download (HTTP)
---

# Models Download (HTTP)

ARW provides HTTP endpoints to manage local models with streaming downloads, live progress via SSE, safe cancel, resume (HTTP Range), and optional SHA‑256 verification.

Updated: 2025-09-12

## Endpoints

- POST `/models/download` — Start or resume a download.
- POST `/models/download/cancel` — Cancel an in‑flight download.
- GET  `/events` — Listen for `Models.DownloadProgress` events.

## Request

POST /models/download

Body:

```
{
  "id": "<model-id>",
  "url": "https://.../model.gguf",
  "provider": "local",  // optional
  "sha256": "<hex>"      // required (fail-closed)
}
```

Behavior:
- Creates a temporary file `{state_dir}/models/<name>.part` and appends chunks.
- On completion, atomically renames to the final filename.
- Honors `Content-Disposition: attachment; filename=...` to pick a server-provided filename (sanitized cross‑platform).
- Verifies the file against `sha256` and removes it on mismatch.
- If a `.part` exists and the server supports HTTP Range, ARW resumes from the saved offset.
  - Uses `If-Range` with previously observed `ETag`/`Last-Modified` to avoid corrupt resumes when the remote file changed.

## Cancel

POST /models/download/cancel

```
{ "id": "<model-id>" }
```

Cancels the active download and removes the partial `.part` file.

## Progress (SSE)

Subscribe to `GET /events` and filter `Models.DownloadProgress` events. Examples:

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
- Codes: `code` provides a stable machine hint for complex statuses (e.g., `admission_denied`, `hard_exhausted`, `disk_insufficient`, `size_limit(_stream)`, `checksum_mismatch`, `canceled_by_user`).
- Budget snapshot: `budget` object with `soft_ms`, `hard_ms`, `spent_ms`, `remaining_*` when available.
- Disk snapshot: `disk` object `{available,total,reserve}` when available.

UI guidance:
- Simple statuses (started/downloading/resumed/complete/canceled) should use compact single icons.
- Complex codes can show a small, subtle icon set (e.g., `lock+timer` for `admission_denied`).
```

## Examples

Start a download (with checksum):

```bash
curl -sS -X POST http://127.0.0.1:8090/models/download \
  -H 'Content-Type: application/json' \
  -d '{"id":"qwen2.5-coder-7b","url":"https://example.com/qwen.gguf","sha256":"<hex>"}'
```

Cancel:

```bash
curl -sS -X POST http://127.0.0.1:8090/models/download/cancel \
  -H 'Content-Type: application/json' \
  -d '{"id":"qwen2.5-coder-7b"}'
```

Resume:
- Re-issue the same `POST /models/download` request. If the server honors `Range: bytes=<offset>-`, ARW resumes from the existing `.part` file.

## Notes
- When `total` is unknown, events may omit it and include only `downloaded`.
- On failure, the model list is updated to `status: "error"` with `error_code` to avoid stuck "downloading" states.
- State directory is shown in `GET /probe`.
- Disk safety: the downloader reserves space to avoid filling the disk. Set `ARW_MODELS_DISK_RESERVE_MB` (default 256) to control the reserved free‑space buffer. If there isn’t enough free space for the download, it aborts with an error event.
- Size caps: set `ARW_MODELS_MAX_MB` (default 4096) to cap the maximum allowed size per download. The cap is enforced using the `Content-Length` when available and during streaming when it isn’t.
- Checksum: when `sha256` is provided, it must be a 64‑char hex string; invalid values are rejected up front.
- Progress payloads can include a budget snapshot (`budget`) and disk info (`disk`) when enabled via env:
  - `ARW_DL_PROGRESS_INCLUDE_BUDGET=1`
  - `ARW_DL_PROGRESS_INCLUDE_DISK=1`
  Related tuning knobs: `ARW_MODELS_MAX_MB`, `ARW_MODELS_DISK_RESERVE_MB`, `ARW_DL_MIN_MBPS`, `ARW_DL_EWMA_ALPHA`, `ARW_DL_SEND_RETRIES`, `ARW_DL_STREAM_RETRIES`, `ARW_DL_IDLE_TIMEOUT_SECS`, `ARW_BUDGET_SOFT_DEGRADE_PCT`.
- Admission checks: when `total` is known, the downloader estimates if it can finish within the remaining hard budget using a throughput baseline `ARW_DL_MIN_MBPS` and a persisted EWMA. If not, it emits `code: "admission_denied"`.
- Idle safety: when no hard budget is set, `ARW_DL_IDLE_TIMEOUT_SECS` applies an idle timeout to avoid hung transfers.
