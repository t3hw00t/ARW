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
  "sha256": "<hex>"      // optional
}
```

Behavior:
- Creates a temporary file `{state_dir}/models/<name>.part` and appends chunks.
- On completion, atomically renames to the final filename.
- When `sha256` is provided, verifies the file and removes it on mismatch.
- If a `.part` exists and the server supports HTTP Range, ARW resumes from the saved offset.

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
- Errors surface in progress events; model list isn’t updated on failure.
- State directory is shown in `GET /probe`.
