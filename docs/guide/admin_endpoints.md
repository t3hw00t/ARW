---
title: Admin Endpoints
---

# Unified Admin Surface
{ .topic-trio style="--exp:.55; --complex:.65; --complicated:.55" data-exp=".55" data-complex=".65" data-complicated=".55" }

The unified `arw-server` binary exposes a single HTTP surface built around the **actions → events → state** triad. Every operation that mutates or inspects the service lives on that surface—there is no `/admin` prefix to keep in sync. New routes are recorded at runtime and streamed into `/about` so that clients can discover the current topology without hard-coding paths.

Updated: 2025-03-08  
Type: How-to

- Service: `arw-server` (default bind `127.0.0.1:8091`)
- Core triad: `POST /actions` (mutations) → `GET /events` (SSE feed) → `GET /state/*` (materialized views)
- Public entrypoints: `/healthz`, `/about`, `/spec/*`, `/catalog/index`, `/catalog/health`
- Supporting surfaces: connectors, config, context, leases, memory, orchestrator, policy, and egress controls (documented below)

<<<<<<< HEAD
Unless noted, examples assume the unified `arw-server` on `http://127.0.0.1:8091`. Use port `8090` when interacting with the legacy `arw-svc` bridge.

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
  "service": "arw-server",
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
    - Keep the service bound to `127.0.0.1` or place behind a TLS proxy
    - Tune rate limits with `ARW_ADMIN_RL` (e.g., `60/60`)
    - Avoid `ARW_DEBUG=1` outside local development
=======
!!! warning "Minimum secure setup"
    - Set `ARW_ADMIN_TOKEN` and require it on every call that mutates configuration, connectors, egress posture, logic units, or policy.
    - Keep the service bound to `127.0.0.1` (or place it behind a TLS reverse proxy with mTLS/OIDC at the edge).
    - Avoid exporting `ARW_DEBUG=1` outside of local experiments; several guardrails disable themselves when debug mode is enabled.
>>>>>>> pr-57

## Authentication

- Header: `Authorization: Bearer <token>` **or** `X-ARW-Admin: <token>`.
- Server toggle: set `ARW_ADMIN_TOKEN` to the expected token; if it is unset or blank, the surface behaves as development/open.
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

`/about` surfaces what the triad recorder knows about the running binary. Each router mount calls `route_get_tag!` / `route_post_tag!` (or their recording counterparts) to push an entry into the in-memory registry. The response exposes that registry alongside build metadata and counts so you can programmatically discover capabilities and stability levels.

Example (truncated for brevity):

```json
{
  "service": "arw-server",
  "version": "0.1.0",
  "http": { "bind": "127.0.0.1", "port": 8091 },
  "docs_url": "https://t3hw00t.github.io/ARW/",
  "security_posture": null,
  "counts": { "public": 59, "admin": 0, "total": 59 },
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
  "perf_preset": { "tier": null, "http_max_conc": 1024, "actions_queue_max": 1024 }
}
```

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
  - `POST /context/assemble` — drive the working-set loop once or stream each iteration (set `stream=true`) to build retrieval context, including diagnostics and coverage metadata.
  - `POST /context/rehydrate` — rehydrate context pointers (currently file heads) with lease-aware guardrails and policy enforcement.
- **Memory writes & advanced retrieval**
  - `POST /memory/put` — insert a memory record (optionally with embeddings/tags) and emit `memory.record.put`.
  - `POST /memory/search_embed` — vector search by embedding.
  - `POST /memory/link` — create a link between memory items.
  - `POST /state/memory/select_hybrid` — hybrid lexical/vector search with optional filters.
  - `POST /memory/select_coherent` — assemble a coherent working set across lanes with optional evidence expansion.
  - `POST /state/memory/explain_coherent` — request explanations for coherent selections (debug-oriented).
- **Connectors**
  - `POST /connectors/register` — write a connector manifest to disk and emit `connectors.registered` (token required).
  - `POST /connectors/token` — store or rotate connector tokens/secrets and emit `connectors.token.updated` (token required).
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
  - `GET /state/orchestrator/jobs` — orchestrator jobs snapshot for mini agents.
- **Connectors**
  - `GET /state/connectors` — connector manifests with secrets scrubbed.
- **Memory & context**
  - `GET /state/memory/select` — textual search across memory lanes (`q`, `mode`, `limit`).
  - `GET /state/memory/links` — list memory links for inspection.
  - `GET /state/memory/recent` — most recent memory inserts per lane.
- **Self introspection**
  - `GET /state/self` — list available self models on disk.
  - `GET /state/self/:agent` — fetch a specific self model JSON snapshot.

## Examples

Setup:

```bash
export ARW_ADMIN_TOKEN=secret123
BASE=http://127.0.0.1:8091  # legacy bridge listens on 8090
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
curl -N "${BASE}/events?replay=5"
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

## Legacy `arw-svc` surface (deprecated)

The classic `arw-svc` bridge still exists for workflows that depend on the legacy debug UI and the historical `/admin/*` namespace. Launch it explicitly with `scripts/start.sh --legacy` on macOS/Linux or `scripts/start.ps1 -Legacy` on Windows. The `/admin` prefix is deprecated; new features ship only on the unified triad surface, and `/about` will continue to advertise every supported endpoint. Keep the bridge isolated and plan migrations toward the `arw-server` routes above.
