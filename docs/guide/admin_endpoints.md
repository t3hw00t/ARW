---
title: Admin Endpoints
---

# Admin Endpoints
{ .topic-trio style="--exp:.5; --complex:.7; --complicated:.6" data-exp=".5" data-complex=".7" data-complicated=".6" }

ARW exposes a unified admin/ops HTTP namespace under `/admin`. All sensitive routes live here so updates can’t miss gating.

Updated: 2025-09-15
Type: How‑to

- Base: `/admin`
- Index (HTML): `/admin`
- Index (JSON): `/admin/index.json`
- Public endpoints (no auth): `/healthz`, `/metrics`, `/spec/*`, `/version`, `/about`

### Public: /about
- Path: `GET /about`
- Returns a small JSON document with service + branding info and a live endpoint index:
  - `name`: "Agent Hub (ARW)"
  - `tagline`: "Your private AI control room that can scale and share when you choose."
  - `description`: one‑paragraph plain‑terms summary
  - `service`: binary id (e.g., `arw-svc`)
  - `version`: semantic version string
  - `role`: current node role
  - `docs_url`: base docs URL if configured
  - `counts`: endpoint counts — `{ public, admin, total }`.
  - `endpoints`: list of known endpoints as strings in the form `"METHOD /path"`.
    - Public endpoints are recorded at router build time (source-of-truth is the runtime recorder).
    - Admin endpoints come from the compile-time registry via `#[arw_admin]` (prevents drift).
    - The list is deduped and sorted.

Example
```json
{
  "name": "Agent Hub (ARW)",
  "tagline": "Your private AI control room that can scale and share when you choose.",
  "description": "Agent Hub (ARW) lets you run your own team of AI helpers on your computer to research, plan, write, and build—while you stay in charge.",
  "service": "arw-svc",
  "version": "0.1.0",
  "role": "Home",
  "docs_url": "https://t3hw00t.github.io/ARW/",
  "counts": { "public": 12, "admin": 48, "total": 60 },
  "endpoints": [
    "GET /healthz",
    "GET /version",
    "GET /spec/openapi.yaml",
    "GET /admin/events",
    "GET /admin/probe"
  ]
}
```

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
- `/admin/tools/cache_stats`: tool Action Cache stats (hit/miss/coalesced, capacity, ttl)
- `/admin/events`: SSE event stream
- `/admin/probe`: effective paths & memory snapshot (read‑only)
- `/admin/memory[/*]`: memory get/apply/save/load/limit
- Quarantine review (planned MVP):
  - `POST /admin/memory/quarantine` — add a quarantined item (provenance, risk markers, evidence score, extractor)
  - `POST /admin/memory/quarantine/admit` — admit (remove) a quarantined item by id
- `/admin/models[/*]`: list/save/load/add/delete/default/download
- `/admin/tools[/*]`: list and run tools
- `/admin/feedback[/*]`: feedback engine state & policy
- `/admin/self_model/propose` (POST): propose a self‑model update; emits `self.model.proposed`
- `/admin/self_model/apply` (POST): apply a proposal; emits `self.model.updated`
- `/admin/state/*`: observations, beliefs, world, intents, actions
  - `/admin/state/models_metrics`: models download counters + EWMA (read‑model). Shape matches the metrics used in `/admin/models/summary`.
  - `/admin/state/route_stats`: per‑route latency/hit/error read‑model
  - `/admin/state/world`: Project Map snapshot (scoped belief graph)
  - `/admin/state/world/select`: top‑K beliefs (claims) with trace
  - `/admin/context/assemble`: minimal context assembly (beliefs + policy/model)
    - Returns `context_preview` (formatted evidence) and `aux.context` (packing metrics)
    - Supports non‑persistent overrides via query params:
      - `context_format` = bullets|jsonl|inline|custom
      - `include_provenance` = true|false
      - `context_item_template` = string (custom format)
      - `context_header` / `context_footer` / `joiner`
      - `context_budget_tokens` / `context_item_budget_tokens`
- `/admin/governor/*`: governor profile & hints
  - Hints include retrieval/formatting knobs: `retrieval_k`, `mmr_lambda`, `compression_aggr`, `vote_k`, `context_budget_tokens`, `context_item_budget_tokens`, `context_format`, `include_provenance`, `context_item_template`, `context_header`, `context_footer`, `joiner`.
- `/admin/hierarchy/*`: negotiation & role/state helpers
- RPU (Regulatory Provenance Unit): trust store
  - `GET /admin/rpu/trust` — redacted trust issuers (id, alg)
  - `POST /admin/rpu/reload` — reload trust store from disk (publishes `rpu.trust.changed`)
  - Gating keys: `rpu:trust:get`, `rpu:trust:reload`

Examples
```bash
# Trust summary
curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  "$BASE/admin/rpu/trust" | jq

# Reload trust (emits rpu.trust.changed)
curl -sS -X POST -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  "$BASE/admin/rpu/reload" | jq

# Watch only trust events (SSE)
curl -N -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  "$BASE/admin/events?prefix=rpu.&replay=5"
```
- World diffs review (planned MVP):
  - `POST /admin/world_diffs/queue` — queue a world diff from collaborators for review
  - `POST /admin/world_diffs/decision` — decide queued diff {apply|reject|defer}
- `/admin/projects/*`: project list/tree/notes
- `/admin/chat[/*]`: chat inspection and send/clear
- Goldens & Experiments
  - `/admin/goldens/list?proj=NAME` — list goldens for a project
  - `/admin/goldens/add` (POST) — add a golden item `{proj,kind:"chat",input:{prompt},expect:{contains|equals|regex}}`
  - `/admin/goldens/run` (POST) — run evaluator `{proj,limit?,temperature?,vote_k?}` (uses retrieval/formatting hints if set)
  - `/admin/experiments/define` (POST) — define variants with knobs
  - `/admin/experiments/run` (POST) — A/B on goldens `{id,proj,variants:["A","B"]}`; emits `experiment.result` and `experiment.winner`
  - `/admin/experiments/activate` (POST) — apply a variant’s knobs to live hints `{id,variant}`
  - `/admin/experiments/list` (GET) — list experiment definitions
  - `/admin/experiments/scoreboard` (GET) — persisted scoreboard (last‑run snapshot per variant)
  - `/admin/experiments/winners` (GET) — persisted winners (last known)
- Patch Safety
  - `/admin/safety/checks` (POST) — static red‑team checks for proposed patches; returns issues (SSRF patterns, prompt‑injection phrasing, secrets markers, permission widenings)
  - `ARW_PATCH_SAFETY=1` — enforce checks in `/admin/patch/apply`
- Distillation
  - `/admin/distill/run` (POST) — run distillation once (beliefs/playbooks/index hygiene); emits `distill.completed`
- `/admin/emit/test`: emit a test event
- `/admin/shutdown`: request shutdown

## Egress (Service Endpoints)

These endpoints live under the public service namespace; use admin token for writes.

- `GET /state/egress/settings` — effective settings (posture and toggles)
- `POST /egress/settings` — update toggles and persist to config (admin‑gated)
- `POST /egress/preview` — dry‑run a URL against policy/guards and return `{ allow|reason }`

See How‑to → Egress Settings and Architecture → Egress Firewall for details.

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

Canonical topic names are defined once in `crates/arw-topics/src/lib.rs` and referenced by the service.

- models.download.progress (progress)
```
{ "id": "qwen2.5-coder-7b", "status": "downloading", "code": "progress", "progress": 42, "downloaded": 12582912, "total": 30000000 }
```

- models.download.progress (complete)
```
{ "id": "qwen2.5-coder-7b", "status": "complete", "code": "complete", "file": "qwen.gguf", "provider": "local", "cas_file": "<sha256>.gguf" }
```

- models.download.progress (error)
```
{ "id": "qwen2.5-coder-7b", "error": "checksum mismatch", "code": "checksum-mismatch", "expected": "<hex>", "actual": "<hex>" }
```

- models.download.progress (canceled)
```
{ "id": "qwen2.5-coder-7b", "status": "canceled", "code": "canceled-by-user" }
```

- models.cas.gc (summary)
```
{ "scanned": 12, "kept": 9, "deleted": 3, "deleted_bytes": 8796093022, "ttl_days": 14 }
```

- egress.preview (pre-offload)
```
{ "id": "qwen2.5-coder-7b", "url": "https://example/model.gguf", "dest": { "host": "example", "port": 443, "protocol": "https" }, "provider": "local", "corr_id": "..." }
```

- egress.ledger.appended (allow)
```
{ "decision": "allow", "reason_code": "models.download", "posture": "off", "project_id": "default", "episode_id": null, "corr_id": "...", "node_id": null, "tool_id": "models.download", "dest": { "host": "example", "port": 443, "protocol": "https" }, "bytes_out": 0, "bytes_in": 1048576, "duration_ms": 1200 }
```

See also: Developer → [Egress Ledger Helper (Builder)](../developer/style.md#egress-ledger-helper-builder)

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
  - Emits standardized `models.download.progress` events.
  
  Example request/response
  ```bash
  curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
    -H 'Content-Type: application/json' \
    -d '{"id":"llama3.1:8b-instruct-q4_K_M","url":"https://example/model.bin","sha256":"7f2e4c0f9b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e","provider":"hf"}' \
    -X POST "$BASE/admin/models/download" | jq
  # {
  #   "ok": true
  # }
  ```
- `POST /admin/models/download/cancel` — Cancel an in‑flight download for `{id}`. Emits `cancel-requested` then `canceled` when complete (or `no-active-job`).
- `POST /admin/models/cas_gc` — Run a one‑off CAS GC sweep: `{ttl_days}`. Emits `models.cas.gc`.
- `GET  /state/models` — Public, read‑only models list.
- `GET  /admin/state/models_hashes` — Admin summary of installed hashes and sizes.

Notes
- Success responses use a consistent envelope `{ ok: true, data: ... }`.
- Errors return RFC‑7807 ProblemDetails with HTTP status codes. Missing services and unexpected errors are mapped to `500` with a structured JSON body.
- `GET  /admin/models/by-hash/:sha256` — Serve a CAS blob by hash (egress‑gated; `io:egress:models.peer`).
- `GET  /admin/state/models_metrics` — Lightweight downloads metrics used for admission checks; returns `{ ewma_mbps: number|null, started, queued, admitted, resumed, canceled, completed, completed_cached, errors, bytes_total }`.
- `GET  /admin/state/egress/ledger` — Last N ledger entries (JSONL → JSON array).
- `GET  /admin/state/egress/ledger/summary` — Summary with optional filters (`since_ms`, `decision`, `reason_code`, `project_id`); returns `{ count, scanned, bytes_in, bytes_out, by_decision, top_reasons, sample }`.
 - `GET  /admin/state/models_metrics` — Read‑model counters `{ started, queued, admitted, resumed, canceled, completed, completed_cached, errors, bytes_total, ewma_mbps }`.
- SSE: `state.read.model.patch` with id=`models_metrics` publishes RFC‑6902 JSON Patches with coalescing.
- `POST /admin/models/concurrency` — Set models download concurrency at runtime. Body: `{ max: number, block?: boolean }`. When `block` is `true` (default), shrinking waits for permits; when `false`, it shrinks opportunistically.
- `GET  /admin/models/concurrency` — Get current concurrency, including `{ configured_max, available_permits, held_permits, hard_cap, pending_shrink? }`.
  - `GET  /admin/models/jobs` — Snapshot of active jobs and inflight hashes for observability.
    
    Example response
    ```json
    {
      "active": [
        { "model_id": "llama3.1:8b-instruct-q4_K_M", "job_id": "dl-5f45c0a1" }
      ],
      "inflight_hashes": [
        "7f2e4c0f9b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e"
      ],
      "concurrency": { "configured_max": 2, "available_permits": 1, "held_permits": 0 }
    }
    ```

See also
- Reference → Models Typed Shapes for the stable response schemas used by these endpoints.

### Tools

- `GET /admin/tools/cache_stats` — Tool Action Cache stats `{ hit, miss, coalesced, entries, ttl_secs, capacity }`.
- Events: `tool.cache` per run `{ id, outcome:hit|miss|coalesced, elapsed_ms, key, digest, age_secs }`.

### Route Stats (Read‑model)

- `GET /admin/state/route_stats` — `{ by_path: { "/path": { hits, errors, ewma_ms, p95_ms, max_ms } } }`.
- SSE: `state.read.model.patch` with id=`route_stats` (coalesced).

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
