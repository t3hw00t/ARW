---
title: Admin Endpoints
---

# Admin Endpoints
## Unified Admin Surface
{ .topic-trio style="--exp:.55; --complex:.65; --complicated:.55" data-exp=".55" data-complex=".65" data-complicated=".55" }

The unified `arw-server` binary exposes an HTTP surface built around the **actions → events → state** triad. Administrative tooling — debug panels, orchestration helpers, policy/config writers — lives under the `/admin/*` namespace so it is easy to secure as a group. The server records every mounted route at build time and streams a live index from `/about`, which keeps clients and docs in sync without hand-maintained tables.

Updated: 2025-09-21
Type: How-to

- Service: `arw-server` (default bind `127.0.0.1:8091`)
- Core triad: `POST /actions` (mutations) → `GET /events` (SSE feed) → `GET /state/*` (materialized views)
- Public entrypoints: `/healthz`, `/about`, `/spec/*`, `/catalog/index`, `/catalog/health`
- Supporting surfaces: connectors, config, context, leases, memory, orchestrator, policy, and egress controls (documented below)

Unless noted, examples assume the unified `arw-server` on `http://127.0.0.1:8091`.

### Public: /about
- Path: `GET /about`
- Returns a small JSON document with service + branding info and a live endpoint index:
  - `name`: "Agent Hub (ARW)"
  - `tagline`: "Your private AI control room that can scale and share when you choose."
  - `description`: one‑paragraph plain‑terms summary
  - `service`: binary id (e.g., `arw-server`)
  - `version`: semantic version string
  - `role`: current node role
  - `docs_url`: base docs URL if configured
  - `counts`: endpoint counts — `{ public, admin, total }` (totals reflect unique endpoints).
  - `endpoints`: list of known endpoints as strings in the form `"METHOD /path"`.
    - Public endpoints are recorded at router build time (source-of-truth is the runtime recorder).
    - Admin endpoints come from the compile-time registry via `#[arw_admin]` (prevents drift) and are merged with the runtime recorder at response time.
    - The list is deduped and sorted.

Example (counts will vary as new endpoints land)
```json
{
  "name": "Agent Hub (ARW)",
  "tagline": "Your private AI control room that can scale and share when you choose.",
  "description": "Agent Hub (ARW) lets you run your own team of AI helpers on your computer to research, plan, write, and build—while you stay in charge.",
  "service": "arw-server",
  "version": "0.1.4",
  "role": "Home",
  "docs_url": "https://t3hw00t.github.io/ARW/",
  "counts": { "public": 62, "admin": 48, "total": 110 },
  "endpoints": [
    "GET /healthz",
    "GET /spec/openapi.yaml",
    "GET /events",
    "GET /admin/probe",
    "GET /admin/debug"
  ]
}
```

!!! warning "Minimum Secure Setup"
    - Set `ARW_ADMIN_TOKEN` and require it on all admin calls
    - Keep the service bound to `127.0.0.1` or place behind a TLS proxy
    - Planned: per-token/IP rate limiter (`ARW_ADMIN_RL=limit/window`) — rely on admin tokens and default concurrency caps until the limiter lands
    - Avoid `ARW_DEBUG=1` outside local development (debug mode is the only time admin endpoints are open without a token)

## Authentication

- Header: `Authorization: Bearer <token>` **or** `X-ARW-Admin: <token>`.
- Server toggle: set `ARW_ADMIN_TOKEN` (or `ARW_ADMIN_TOKEN_SHA256`) to the expected token; when neither variable is set the surface stays locked unless `ARW_DEBUG=1` is also present.
- Mutating endpoints that require the token today include:
  - Configuration and schema helpers (`POST /patch/*`).
  - Connector lifecycle (`POST /connectors/register`, `POST /connectors/token`).
  - Egress posture updates (`POST /egress/settings`).
  - Policy reloads (`POST /policy/reload`).
  - Logic unit management (`POST /logic-units/*`).
  - Any helper that calls out to the filesystem (for example `POST /context/rehydrate`).
- Set `ARW_ACTIONS_QUEUE_MAX` to gate the number of queued actions; requests beyond the limit receive HTTP `429`.

## Public entrypoints

- `GET /healthz` — simple `{ "ok": true }` readiness probe.
- `GET /about` — runtime metadata, perf hints, and the live endpoint index.
- Specs: `GET /spec/openapi.yaml`, `GET /spec/asyncapi.yaml`, `GET /spec/mcp-tools.json`, `GET /spec/schemas/:file`, `GET /spec/index.json`.
- Catalog helpers: `GET /catalog/index`, `GET /catalog/health`.

### `/about` and endpoint discovery

`/about` surfaces what the triad recorder knows about the running binary. Each router mount calls `route_get_tag!` / `route_post_tag!` (or their recording counterparts) to push an entry into the in-memory registry. At response time the service merges that runtime list with the compile-time admin registry so clients can distinguish public and privileged surfaces while the counts remain accurate.

Example (truncated for brevity):

```json
{
  "service": "arw-server",
  "version": "0.1.4",
  "http": { "bind": "127.0.0.1", "port": 8091 },
  "docs_url": "https://t3hw00t.github.io/ARW/",
  "security_posture": null,
  "counts": { "public": 62, "admin": 48, "total": 110 },
  "endpoints": [
    "GET /healthz",
    "GET /about",
    "POST /actions",
    "GET /state/actions",
    "GET /events"
  ],
  "endpoints_meta": [
    { "method": "GET", "path": "/healthz", "stability": "stable" },
    { "method": "POST", "path": "/actions", "stability": "beta" }
  ],
  "perf_preset": { "tier": "balanced", "http_max_conc": 1024, "actions_queue_max": 1024 }
}
```

Preset values adapt to the host; the example above reflects the `balanced` tier (autodetected on mid-range laptops). Low-power hosts default closer to 256, while workstations can scale to 16384.

## Triad surface

The triad groups operations by intent:

| Plane  | Purpose | Representative endpoints |
| ------ | ------- | ------------------------ |
| Actions | Submit work and mutate state | `POST /actions`, `POST /leases`, `POST /egress/settings`, `POST /patch/apply` |
| Events | Subscribe to live changes | `GET /events` |
| State  | Read materialized views and history | `GET /state/actions`, `GET /state/egress`, `GET /state/config` |

### Actions plane (mutations)

- **Action queue**
  - `POST /actions` — enqueue a new action; policy and lease checks fire before the job enters the queue.
  - `GET /actions/:id` — retrieve the current state, IO payloads, and timestamps for an action.
  - `POST /actions/:id/state` — transition an action (`queued|running|completed|failed`), emitting bus events for downstream consumers.
- **Leases**
  - `POST /leases` — mint an expiring capability lease (for example `context:rehydrate:file`), returning the id and expiry timestamp.
- **Policy controls**
  - `POST /policy/reload` — hot-reload the policy engine from environment configuration; emits `policy.reloaded` and requires the admin token.
  - `POST /policy/simulate` — evaluate a prospective ABAC request without persisting state; returns the decision payload.
- **Egress posture**
  - `POST /egress/settings` — patch the effective posture/allowlist/toggles, validate against `spec/schemas/egress_settings.json`, persist a snapshot, and publish `egress.settings.updated` (token required).
  - `POST /egress/preview` — dry-run a URL against the allowlist, posture, and lease policy to see whether it would be allowed.
- **Configuration & schema helpers**
  - `POST /patch/apply` — apply one or more JSON merge/set patches to the runtime config, validate against an optional schema, snapshot, and emit a config patch event.
  - `POST /patch/revert` — roll back to an earlier snapshot by id (requires token).
  - `POST /patch/validate` — validate payloads against schemas without applying them.
  - `POST /patch/infer_schema` — attempt to infer which schema/pointer applies to a dotted path and return guidance.
- **Logic units**
  - `POST /logic-units/install` — register a logic unit package on disk (token required).
  - `POST /logic-units/apply` — activate a logic unit version across the runtime.
  - `POST /logic-units/revert` — roll back to a previous logic unit snapshot.
- **Context assembly**
  - `POST /context/assemble` — drive the working-set loop once or stream each iteration (set `stream=true`) to build retrieval context, including diagnostics and coverage metadata. The request body accepts `slot_budgets` (map of slot → max items) so you can reserve space for instructions, plan, safety, etc.; the response surfaces `slots.counts` and `slots.budgets` to highlight gaps such as `slot_underfilled:instructions` in coverage.
  - `POST /context/rehydrate` — rehydrate context pointers (currently file heads) with lease-aware guardrails and policy enforcement.
- **Memory writes & advanced retrieval**
  - `POST /actions (memory.upsert)` — insert or merge a memory item; emits `memory.item.upserted` and updates `/state/memory`.
  - `POST /actions (memory.search)` — hybrid lexical/vector search with filters and RRF/MMR metadata.
  - `POST /actions (memory.pack)` — build a context pack respecting token/slot budgets; journals decisions via `memory.pack.journaled`.
  - `POST /admin/memory/apply` — lightweight helper to upsert a record directly via the kernel (admin token required). Returns `{ id, record, applied }` where `record` mirrors the canonical memory item (id/lane/kind/key/tags/hash/value/ptr) and `applied` includes the observability metadata broadcast on `memory.applied` (`source`, `value_preview`, `value_preview_truncated`, `value_bytes`, `applied_at`).
  - `GET /admin/memory` — quick snapshot of recent records (supports `lane` and `limit` filters).
  - Legacy `/memory/*` selectors have been removed; use the action-based flow and `GET /state/memory/recent` for inspection.
- **Connectors**
  - `POST /connectors/register` — write a connector manifest to disk and emit `connectors.registered` (token required).
  - `POST /connectors/token` — store or rotate connector tokens/secrets and emit `connectors.token.updated` (token required). Connector scopes require matching capability leases before use; missing scopes surface `connector lease required`.
- **Orchestrator**
  - `POST /orchestrator/mini_agents/start_training` — kick off training for mini agents; the read model exposes progress via `/state/orchestrator/jobs`.

### Events plane (observation)

- `GET /events` — Server-Sent Events stream of every bus publication. Supports `after`, `replay`, and `prefix` query parameters along with `Last-Event-ID` headers for resume. Set `ARW_EVENTS_SSE_MODE=ce-structured` to emit CloudEvents JSON instead of envelopes.

### State plane (materialized views)

- **Actions & activity**
  - `GET /state/actions` — recent actions (capped by `limit`, default 200).
  - `GET /state/episodes` — roll up recent events by correlation id for quick timeline inspection.
  - `GET /state/route_stats` — per-route counters plus bus metrics.
  - `GET /state/contributions` — last 200 contribution entries.
- **Leases & policy**
  - `GET /state/leases` — active leases with capability, scope, and expiry.
  - `GET /state/policy` — current policy snapshot (rules, defaults, capabilities).
- **Egress**
  - `GET /state/egress` — recent ledger entries (size controlled by `limit`).
  - `GET /state/egress/settings` — effective posture, allowlist, proxy toggle, and ledger state derived from env vars.
- **Models**
  - `GET /state/models` — models metadata from `state/models.json` (with defaults when the file is absent).
- **Configuration & schemas**
  - `GET /state/config` — effective configuration tree (post-patch).
  - `GET /state/config/snapshots` — snapshot history (id + metadata).
  - `GET /state/config/snapshots/:id` — retrieve a specific snapshot by id.
  - `GET /state/schema_map` — discover schema references indexed by dotted path.
- **Logic units & orchestrator**
  - `GET /logic-units` — installed logic units (read-only catalog).
  - `GET /state/logic_units` — read-model summary of logic unit jobs/status.
- `GET /state/orchestrator/jobs` — orchestrator jobs snapshot for mini agents (each entry now includes `status_slug` + `status_label` so dashboards can share the canonical vocabulary).
- **Connectors**
  - `GET /state/connectors` — connector manifests with secrets scrubbed.
- **Memory & context**
  - `GET /state/memory` — SSE stream emitting `memory.snapshot` and `memory.patch` events (JSON Patches plus live snapshot).
  - `GET /state/memory/recent` — snapshot of recent records (lane/limit filters) with `generated` and `generated_ms` timestamps for freshness checks.
- **Self introspection**
  - `GET /state/self` — list available self models on disk.
  - `GET /state/self/:agent` — fetch a specific self model JSON snapshot.

## Examples

Setup:

```bash
export ARW_ADMIN_TOKEN=secret123
BASE=http://127.0.0.1:8091
AUTH=(-H "Authorization: Bearer $ARW_ADMIN_TOKEN")
```

Queue an action:

```bash
curl -sS "${BASE}/actions" \
  "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"kind":"demo.echo","input":{"message":"hello"}}' | jq
```

Inspect recent actions:

```bash
curl -sS "${BASE}/state/actions?limit=5" | jq '.items[] | {id,kind,state}'
```

Dry-run an egress request:

```bash
curl -sS "${BASE}/egress/preview" \
  "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"url":"https://example.com/model.gguf","method":"GET"}' | jq
```

Stream events (replay the last 5 first):

```bash
curl -N -H "Authorization: Bearer $ARW_ADMIN_TOKEN" "${BASE}/events?replay=5"
```

### Event samples

- `actions.submitted`

  ```json
  {
    "time": "2025-03-08T18:22:11.112Z",
    "kind": "actions.submitted",
    "payload": { "id": "act-123", "kind": "demo.echo", "status": "queued" }
  }
  ```

- `egress.ledger.appended`

  ```json
  {
    "time": "2025-03-08T18:25:41.009Z",
    "kind": "egress.ledger.appended",
    "payload": {
      "decision": "allow",
      "reason": "preview",
      "host": "example.com",
      "protocol": "https"
    }
  }
  ```

## Security guidance

- Keep `ARW_ADMIN_TOKEN` secret and rotate regularly; many high-impact helpers reject requests without it.
- Place the service behind TLS and IP allowlists when exposing it beyond localhost.
- Enable `ARW_EGRESS_LEDGER_ENABLE=1` to persist outbound decisions and audit them via `/state/egress`.
- Use leases to scope dangerous operations (`POST /leases` → `context:rehydrate:file` capability) and require the lease before invoking filesystem helpers.

## Unified admin routes

Administrative tooling now lives entirely inside `arw-server`. Enable `ARW_DEBUG=1` when you need the browser UI surfaces.
