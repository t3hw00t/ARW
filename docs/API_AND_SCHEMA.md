---
title: API and Schema
---

# API and Schema

Updated: 2025-09-06.

See also: [Glossary](GLOSSARY.md), [Configuration](CONFIGURATION.md)

Explore the API
Set a base URL and (optionally) an admin token for gated endpoints.

```bash
export BASE=http://127.0.0.1:8090
export ARW_ADMIN_TOKEN=secret   # if set on the server
H() { curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" "$@"; }
```

Quick checks
```bash
curl -sS "$BASE/healthz"
H "$BASE/introspect/tools" | jq '.[0:5]'
```

Schemas and specs
```bash
# A specific tool schema (example id)
H "$BASE/introspect/schemas/memory.probe@1.0.0" | jq

# OpenAPI / AsyncAPI / MCP tool catalog
curl -sS "$BASE/spec/openapi.yaml" | head -n 20
curl -sS "$BASE/spec/asyncapi.yaml" | head -n 20
curl -sS "$BASE/spec/mcp-tools.json" | jq 'keys'
```

Events (SSE)
```bash
curl -N "$BASE/events?replay=10"
```

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
- In the Debug UI (`/debug`, set `ARW_DEBUG=1`), the Tools panel exercises example tools and shows emitted `Tool.Ran` events.
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

New endpoints (introspection & feedback)
- `GET /introspect/stats`: returns event totals and per‑route metrics (hits, errors, EWMA, last/max ms).
- `POST /feedback/signal`: record a signal `{ kind, target, confidence, severity, note }`.
- `POST /feedback/analyze`: produce suggestions from signals and stats.
- `POST /feedback/apply`: apply a suggestion `{ id }` (updates hints/profile/memory limit conservatively).
- `GET /feedback/state`: feedback state (signals, suggestions, auto_apply).
- `POST /feedback/auto`: toggle `auto_apply`.
- `POST /feedback/reset`: clear signals & suggestions.

## Security Notes
- Sensitive endpoints are gated; see Developer Security Notes.

#[arw_tool] macro derives Schemas, Tool impl, registry entry, MCP metadata.

Validate input → policy check → invoke → emit events → return.

## HTTP & WS Surface

GET /introspect/tools

GET /introspect/schemas/{tool_id}

POST /tools/{tool_id}:invoke

GET /probe?task_id=...&step=...

WS /events

GET /spec/openapi.yaml

GET /spec/asyncapi.yaml

GET /spec/mcp-tools.json

## Connections (New)

GET /connectors — list available connector types/providers

POST /connectors/register — add/register a custom connector (policy-gated)

GET /connections — list connections in the registry

POST /connections — create a connection (disabled by default, optional dry-run)

GET /connections/{id}

PATCH /connections/{id} — update tuning (rate limit, retry, QoS, notes)

POST /connections/{id}/toggle — enable/disable

POST /connections/{id}/test — active health check + trace

POST /connections/{id}/set-policy — bind a policy id

POST /connections/{id}/bind-secret — bind auth/secret reference

DELETE /connections/{id}

GET /links — list active links (connection ↔ service bindings)

POST /links — create a link (policy checked), optional auto-enable

DELETE /links/{id}

## Events (AsyncAPI)

Versioned event types; include time, task_id, span_id, severity.

Connections: ConnectionAdded, ConnectionUpdated, ConnectionRemoved, ConnectionPolicyChanged, ConnectionSecretBound

Links: LinkUp, LinkDown, LinkHealthChanged, RateLimitHit, BackoffApplied

## MCP Bridge

All registered tools appear to MCP clients with the same ids and schemas.

Admin MCP tools for connections: conn.list, conn.create, conn.update, conn.toggle, conn.test.

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
