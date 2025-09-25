---
title: API and Schema
---

# API and Schema
{ .topic-trio style="--exp:.9; --complex:.6; --complicated:.5" data-exp=".9" data-complex=".6" data-complicated=".5" }

Updated: 2025-09-22
Type: Reference

See also: [Glossary](GLOSSARY.md), [Configuration](CONFIGURATION.md)

Explore the API
Set a base URL and (optionally) an admin token for gated endpoints.

```bash
export BASE=http://127.0.0.1:8091
export ARW_ADMIN_TOKEN=secret   # optional admin token for /admin and /spec routes
H() { curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" "$@"; }
```

Quick probes
```bash
curl -sS "$BASE/about" | jq '.service, .version'
curl -sS "$BASE/state/route_stats" | jq '.routes | to_entries? // [] | .[:5]'
curl -sS "$BASE/spec/index.json" | jq '.entries | map(.path)'
curl -sS "$BASE/spec/health" | jq
```

Schemas and specs
```bash
# OpenAPI / AsyncAPI / MCP tool catalog
curl -sS "$BASE/spec/openapi.yaml" | head -n 20
curl -sS "$BASE/spec/asyncapi.yaml" | head -n 20
curl -sS "$BASE/spec/mcp-tools.json" | jq 'keys'

# Self‑Model JSON Schema (static)
cat spec/schemas/self_model.json | jq '.title,.description'

# Egress schemas (planned)
cat spec/schemas/policy_network_scopes.json | jq '.title,.description'
cat spec/schemas/egress_ledger.json | jq '.title,.description'

# Memory & World schemas (planned)
cat spec/schemas/memory_quarantine_entry.json | jq '.title,.description'
cat spec/schemas/world_diff_review_item.json | jq '.title,.description'
 
# Models manifest (CAS)
cat spec/schemas/model_manifest.json | jq '.title,.required'
cat spec/schemas/secrets_redaction_rule.json | jq '.title,.description'
cat spec/schemas/archive_unpack_policy.json | jq '.title,.description'
cat spec/schemas/dns_anomaly_event.json | jq '.title,.description'
```

Events (SSE)
```bash
curl -N "$BASE/events?replay=10"
```

The stream opens with a `service.connected` envelope that carries `request_id`, `resume_from`, and replay metadata before historical events (when requested) and live traffic.

!!! warning "Security"
    Many `introspect/*`, `feedback/*`, `tools/*`, and related endpoints are gated. In production, set `ARW_ADMIN_TOKEN` on the service and include `X-ARW-Admin` in requests. See: guide/security_hardening.md

## Goals

Single source of truth for operations (tools), HTTP/WS APIs, MCP tools and docs.

JSON Schema 2020-12 for all inputs/outputs/errors.

Auto-generation of OpenAPI 3.1 (HTTP), AsyncAPI 2.x (events), MCP tool catalogs.

Backward-compatible evolution, RFC 7807 error taxonomy, doc-tests.

## Foundations

JSON Schema 2020-12; OpenAPI 3.1; AsyncAPI 2.x.

RFC 7807 Problem Details errors.

W3C Trace Context + OpenTelemetry.

## UI Cross‑Reference
- In the Debug UI (`/admin/debug`, set `ARW_DEBUG=1`), the Tools panel exercises example tools and shows emitted `tool.ran` events.
- Click the small “?” next to Tools for a tip and a link back to this page.

## Directories
/spec/

openapi.yaml

asyncapi.yaml

mcp-tools.json

schemas/ (per operation & model, generated)

## Operations

OperationId = "<tool_id>@<semver>" (e.g., memory.probe@1.0.0)

Each operation declares: Input, Output, Error types; capabilities; stability (stable/experimental/deprecated).

## Declaration Style (Rust)

New endpoints (introspection, feedback, distill)
- `GET /admin/introspect/stats`: returns event totals and per‑route metrics (hits, errors, EWMA, last/max ms).
- `POST /admin/feedback/signal`: record a signal `{ kind, target, confidence, severity, note }`.
- `POST /admin/feedback/analyze`: produce suggestions from signals and stats.
- `POST /admin/feedback/apply`: apply a suggestion `{ id }` (updates hints/profile/memory limit conservatively).
- `GET /admin/feedback/state`: feedback state (signals, suggestions, auto_apply).
- `POST /admin/feedback/auto`: toggle `auto_apply`.
- `POST /admin/feedback/reset`: clear signals & suggestions.
- `GET /admin/feedback/suggestions`: latest suggestions snapshot (versioned).
- `GET /admin/feedback/updates?since=N`: return suggestions when the version advances.
- `GET /admin/feedback/policy`: effective caps and bounds for auto/apply decisions.
- `GET /admin/feedback/versions`: available feedback_engine snapshots on disk.
- `POST /admin/feedback/rollback?to=N`: restore a previous snapshot (or the `.bak` when omitted).
- `POST /admin/distill`: run an on-demand distillation pass (playbooks snapshot, beliefs export, world version hygiene).

## Security Notes
- Sensitive endpoints are gated; see Developer Security Notes.

#[arw_tool] macro derives Schemas, Tool impl, registry entry, MCP metadata.

Validate input → policy check → invoke → emit events → return.

## HTTP & WS Surface

- `GET /healthz`
- `GET /about`
- `GET /events`
- `POST /actions`
- `GET /actions/:id`
- `POST /actions/:id/state`
- `GET /state/episodes`
- `GET /state/route_stats`
- `GET /state/actions`
- `GET /state/contributions`
- `GET /state/cluster`
- `GET /admin/goldens/list`, `POST /admin/goldens/add`, `POST /admin/goldens/run`
- `POST /admin/experiments/define`, `POST /admin/experiments/run`, `POST /admin/experiments/activate`, `GET /admin/experiments/list`, `GET /admin/experiments/scoreboard`, `GET /admin/experiments/winners`, `POST /admin/experiments/start`, `POST /admin/experiments/stop`, `POST /admin/experiments/assign`
- `POST /leases`
- `GET /state/leases`
- `GET /state/egress`
- `GET /state/egress/settings`
- `POST /egress/settings`
- `POST /egress/preview`
- `GET /state/policy`
- `POST /policy/reload`
- `POST /policy/simulate`
- `GET /state/models`
- `GET /spec/index.json`
- `GET /spec/openapi.yaml`
- `GET /spec/asyncapi.yaml`
- `GET /spec/mcp-tools.json`
- `GET /spec/schemas/{file}`
- `GET /catalog/index`
- `GET /catalog/health`

## Admin Surface (`/admin/*`)

The unified `arw-server` exposes the historical admin APIs directly. Set
`ARW_ADMIN_TOKEN` (or use the `H` helper above) to send the `X-ARW-Admin`
header when calling these endpoints.

Admin probes
```bash
H "$BASE/admin/tools" | jq '.items[0:5]'
H "$BASE/admin/introspect/schemas/memory.probe@1.0.0" | jq
```

Admin event stream (SSE)
```bash
curl -N -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" "$BASE/events?replay=10"
```

Key admin endpoints
- `GET /admin/tools`
- `GET /admin/introspect/schemas/{tool_id}`
- `GET /admin/probe?task_id=...&step=...`
- `SSE /events`

### Models Admin Endpoints

See the Admin Endpoints guide for details and examples. Summary:

- `POST /admin/models/download` — start/resume a download `{id,url,sha256,provider?,budget?}`.
- `POST /admin/models/download/cancel` — cancel an in-flight download for `{id}`.
- `POST /admin/models/cas_gc` — CAS GC once `{ttl_days}`; emits `models.cas.gc`.
- `GET  /admin/models/by-hash/:sha256` — serve a CAS blob by sha256 (egress-gated).
- `GET  /state/models_hashes` — list installed model hashes and sizes.
- `GET  /state/models_metrics` — Lightweight metrics `{ ewma_mbps, started, queued, admitted, resumed, canceled, completed, completed_cached, errors, bytes_total }`.

Admin events (AsyncAPI)
- `models.download.progress`: standardized progress/errors with optional `budget` and `disk`.
- `models.manifest.written`, `models.cas.gc`, `models.changed`, `models.refreshed`.
- Egress: `egress.preview`, `egress.ledger.appended`.

## Connectors (Shipped)

- `GET /state/connectors` — list connector manifests (secrets elided).
- `POST /connectors/register` — register a connector (admin token required).
- `POST /connectors/token` — set tokens/refresh tokens for an existing connector (admin token required).

Future “Connection Manager” surfaces (dedicated `/connections` and `/links` routes) remain on the roadmap and will land with a guarded flag once implementation stabilises. Until then, integrations rely on registered connectors plus capability leases.

## Events (AsyncAPI)

Versioned event types; include time, task_id, span_id, severity.

Connections/links events are planned to accompany the future manager; today the event bus focuses on connector registration and token updates (`connectors.registered`, `connectors.token.updated`).

## MCP Bridge

All registered tools appear to MCP clients with the same ids and schemas.

Admin MCP tooling for the connection manager will ship alongside the HTTP surfaces. Current MCP exports match the connector endpoints above.

## Pagination & IDs

UUID v4 ids; cursors are base64url tokens. Consistent Page<T> helpers in arw-protocol.

## Errors (Problem Details)

{ type, title, status, detail, instance, trace_id, code }

Codes include: validation_failed, policy_denied, timeout, not_found, conflict, unavailable, rate_limited, internal_error.

## Schemas (High Level)

Connector: { id, kind (http|ws|mcp|local), name, capabilities[], version }

Connection: { id, connectorId, target, status (disabled|enabled|error|healthy|degraded), rateLimit, concurrency, retry, backoff, qos, policyId, secretRef, tags[], notes, createdAt, updatedAt }

Link: { id, connectionId, serviceId, status, health { ok, latencyMs, errors[] }, createdAt, updatedAt }

## Doc Pipeline (CI)

arw-docgen aggregates registries → generates /spec artifacts.

Doc-tests execute embedded examples against a local test server.

Schema compatibility guard enforces semver bumps on breaking changes.

Generated specs are authoritative; sample clients from OpenAPI must compile and pass doc‑tests in CI.

## Deprecation

stability=deprecated; maintain ≥2 minor releases; emit Deprecation header with link.

## Extensibility

Third-party plugins (Rust or WASI) use #[arw_tool]; once linked, they appear in all surfaces (policy-gated).
## Read‑Models (New)

GET /state/logic_units

GET /state/experiments

GET /state/runtime_matrix

GET /state/episode/{id}/snapshot

GET /state/policy
