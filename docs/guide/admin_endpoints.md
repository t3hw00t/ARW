---
title: Admin Endpoints
---

# Admin Endpoints
{ .topic-trio style="--exp:.5; --complex:.7; --complicated:.6" data-exp=".5" data-complex=".7" data-complicated=".6" }

ARW exposes a unified admin/ops HTTP namespace under `/admin`. All sensitive routes live here so updates can’t miss gating.

Updated: 2025-09-12

- Base: `/admin`
- Index (HTML): `/admin`
- Index (JSON): `/admin/index.json`
- Public endpoints (no auth): `/healthz`, `/metrics`, `/spec/*`, `/version`, `/about`

!!! warning "Minimum Secure Setup"
    - Set `ARW_ADMIN_TOKEN` and require it on all admin calls
    - Keep the service bound to `127.0.0.1` or place behind TLS proxy
    - Tune rate limits with `ARW_ADMIN_RL` (e.g., `60/60`)
    - Avoid `ARW_DEBUG=1` outside local development

## Authentication

- Header: `X-ARW-Admin: <token>`
- Env var: set `ARW_ADMIN_TOKEN` to the expected token value for the service.
- Local dev: set `ARW_DEBUG=1` to allow admin access without a token.
- Optional gating capsule: send JSON in `x-arw-gate` header to adopt a gating context for the request.

Rate limiting:
- Env var `ARW_ADMIN_RL` as `limit/window_secs` (default `60/60`).

## Common Admin Paths

- `/admin/introspect/tools`: list available tools
- `/admin/introspect/schemas/{id}`: schema for a known tool id
- `/admin/introspect/stats`: runtime & route stats (JSON)
- `/admin/events`: SSE event stream
- `/admin/probe`: effective paths & memory snapshot (read‑only)
- `/admin/memory[/*]`: memory get/apply/save/load/limit
- Quarantine review (planned MVP):
  - `POST /admin/memory/quarantine` — add a quarantined item (provenance, risk markers, evidence score, extractor)
  - `POST /admin/memory/quarantine/admit` — admit (remove) a quarantined item by id
- `/admin/models[/*]`: list/save/load/add/delete/default/download
- `/admin/tools[/*]`: list and run tools
- `/admin/feedback[/*]`: feedback engine state & policy
- `/admin/self_model/propose` (POST): propose a self‑model update; emits `SelfModel.Proposed`
- `/admin/self_model/apply` (POST): apply a proposal; emits `SelfModel.Updated`
- `/admin/state/*`: observations, beliefs, world, intents, actions
  - `/admin/state/world`: Project Map snapshot (scoped belief graph)
  - `/admin/state/world/select`: top‑K beliefs (claims) with trace
  - `/admin/context/assemble`: minimal context assembly (beliefs + policy/model)
- `/admin/governor/*`: governor profile & hints
- `/admin/hierarchy/*`: negotiation & role/state helpers
- World diffs review (planned MVP):
  - `POST /admin/world_diffs/queue` — queue a world diff from collaborators for review
  - `POST /admin/world_diffs/decision` — decide queued diff {apply|reject|defer}
- `/admin/projects/*`: project list/tree/notes
- `/admin/chat[/*]`: chat inspection and send/clear
- `/admin/emit/test`: emit a test event
- `/admin/shutdown`: request shutdown

Note: Depending on build flags and environment, some endpoints may be unavailable or no‑op.

## Examples

Setup:

```bash
export ARW_ADMIN_TOKEN=secret123
BASE=http://127.0.0.1:8090
AH() { curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" "$@"; }
```

- List tools
```bash
AH "$BASE/admin/tools" | jq
```

- Run a tool
```bash
curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"id":"math.add","input":{"a":2,"b":3}}' \
  "$BASE/admin/tools/run" | jq
```

- Apply memory item (accepted)
```bash
curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"kind":"episodic","value":{"note":"hello"}}' \
  -X POST "$BASE/admin/memory/apply" -i
```

- Enqueue a task
```bash
curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"kind":"math.add","payload":{"a":1,"b":4}}' \
  -X POST "$BASE/admin/tasks/enqueue" | jq
```

- Stream events (SSE)
```bash
curl -N -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" "$BASE/admin/events?replay=10"
```

### Event Examples

- Models.DownloadProgress (progress)
```
{ "id": "qwen2.5-coder-7b", "status": "downloading", "code": "progress", "progress": 42, "downloaded": 12582912, "total": 30000000 }
```

- Models.DownloadProgress (complete)
```
{ "id": "qwen2.5-coder-7b", "status": "complete", "code": "complete", "file": "qwen.gguf", "provider": "local", "cas_file": "<sha256>.gguf" }
```

- Models.DownloadProgress (error)
```
{ "id": "qwen2.5-coder-7b", "error": "checksum mismatch", "code": "checksum_mismatch", "expected": "<hex>", "actual": "<hex>" }
```

- Models.DownloadProgress (canceled)
```
{ "id": "qwen2.5-coder-7b", "status": "canceled", "code": "canceled_by_user" }
```

- Models.CasGc (summary)
```
{ "scanned": 12, "kept": 9, "deleted": 3, "deleted_bytes": 8796093022, "ttl_days": 14 }
```

- Egress.Preview (pre-offload)
```
{ "id": "qwen2.5-coder-7b", "url": "https://example/model.gguf", "dest": { "host": "example", "port": 443, "protocol": "https" }, "provider": "local", "corr_id": "..." }
```

- Egress.Ledger.Appended (allow)
```
{ "decision": "allow", "reason_code": "models.download", "posture": "off", "project_id": "default", "episode_id": null, "corr_id": "...", "node_id": null, "tool_id": "models.download", "dest": { "host": "example", "port": 443, "protocol": "https" }, "bytes_out": 0, "bytes_in": 1048576, "duration_ms": 1200 }
```

## OpenAPI

- `/spec/openapi.yaml` provides an OpenAPI document for many admin endpoints (includes Models admin routes).
- The `/admin` index is the authoritative, live source of admin paths from the running binary.

## Security Guidance

- Keep `ARW_ADMIN_TOKEN` secret and rotate routinely.
- Avoid `ARW_DEBUG=1` outside local dev.
- Place ARW behind a reverse proxy with TLS and IP allowlists where possible.
- Consider additional auth at the proxy (mTLS, OIDC) for defense in depth.
Quarantine an item (example)
```bash
curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"project_id":"demo","content_type":"text/html","content_preview":"<html>...</html>","provenance":"https://example.com","risk_markers":["html","script"],"evidence_score":0.3}' \
  -X POST "$BASE/admin/memory/quarantine" | jq
```

World diff queue/decision (example)
```bash
curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"project_id":"demo","from_node":"peer-1","summary":"update beliefs","changes":[{"op":"add","path":"/beliefs/x","value":1}]}' \
  -X POST "$BASE/admin/world_diffs/queue" | jq

curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"id":"<id-from-queue>","decision":"apply","note":"looks good"}' \
  -X POST "$BASE/admin/world_diffs/decision" | jq
```
### Models

- `POST /admin/models/download` — Start or resume a model download.
  - Body: `{id,url,sha256,provider?,budget?}` where `budget` can override `{soft_ms,hard_ms,class}` for this request.
  - Requires a 64‑char hex `sha256`.
  - Emits `Models.Download` (compat) and standardized `Models.DownloadProgress` events.
- `POST /admin/models/download/cancel` — Cancel an in‑flight download for `{id}`. Emits `cancel-requested` then `canceled` when complete (or `no-active-job`).
- `POST /admin/models/cas_gc` — Run a one‑off CAS GC sweep: `{ttl_days}`. Emits `Models.CasGc`.
- `GET  /state/models` — Public, read‑only models list.
- `GET  /admin/state/models_hashes` — Admin summary of installed hashes and sizes.
- `GET  /admin/models/by-hash/:sha256` — Serve a CAS blob by hash (egress‑gated; `io:egress:models.peer`).
- `GET  /admin/models/downloads_metrics` — Lightweight downloads metrics used for admission checks; returns `{ ewma_mbps: number|null }`.

#### Models Manifest

After a successful download and verification, ARW writes a per‑ID manifest next to the CAS store: `{state_dir}/models/<id>.json`.

Example:
```
{
  "id": "qwen2.5-coder-7b",
  "file": "<sha256>.gguf",
  "name": "original_name.gguf",
  "path": "/path/to/state/models/by-hash/<sha256>.gguf",
  "url": "https://example.com/model.gguf",
  "sha256": "<64-hex>",
  "cas": "sha256",
  "bytes": 123456789,
  "provider": "local",
  "verified": true
}
```

Schema: see `spec/schemas/model_manifest.json`.

Notes
- Downloads promote into CAS under `{state_dir}/models/by-hash/<sha256>[.<ext>]` and write a per‑ID manifest `{state_dir}/models/<id>.json`.
- When `ARW_DL_PREFLIGHT=1`, a HEAD preflight enforces `ARW_MODELS_MAX_MB` and optional `ARW_MODELS_QUOTA_MB` before transfer.
- See Guide → Models Download for event schema, budgets, progress, and error codes.
