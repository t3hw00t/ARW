# API Reference

Updated: 2025-09-15
Type: Reference

Microsummary: Public endpoints, admin surfaces, specs, and eventing. Stable/experimental flags are surfaced in specs; deprecations emit standard headers.

- Specs in repo: `spec/openapi.yaml`, `spec/asyncapi.yaml`, `spec/mcp-tools.json`
- Specs at runtime: `GET /spec/openapi.yaml`, `GET /spec/asyncapi.yaml`, `GET /spec/mcp-tools.json`, `GET /spec/schemas/{file}.json`, `GET /spec/index.json`
- Catalog: `GET /catalog/index` (YAML) and `GET /catalog/health` (JSON)
- Auth: Local‑only by default; for admin endpoints set `ARW_ADMIN_TOKEN` and send `Authorization: Bearer <token>` or `X-ARW-Admin`.

Endpoints (selected)
- `GET /healthz`: service health, returns `ok` when ready.
- `GET /debug`: Debug UI (when `ARW_DEBUG=1`).
- `GET /events`: SSE for live updates (unified server). Supports `?replay=N`, `?after=<row_id>`, and `Last-Event-ID` alias. See How‑to → Subscribe to Events (SSE).
- `GET /admin/events`: SSE (legacy service) with `?replay`.
- `GET /state/*`: read‑models (observations, beliefs, world, intents, actions, episodes, self/{agent}).
- `GET /about`: service metadata with endpoints index and counts
  - Fields: `service`, `version`, `http`, `docs_url?`, `security_posture?`, `counts`, `endpoints[]`, `endpoints_meta[]`
  - `endpoints_meta[]` items include `{ method, path, stability }` for curated routes.

Semantics
- status vs code: RFC 7807 ProblemDetails for errors; otherwise endpoint‑specific JSON.
- pagination/filtering: available on selected read‑models (e.g., `/state/models_hashes` supports `limit`, `offset`, `provider`, `sort`, `order`).
- stability: experimental → beta → stable → deprecated → sunset (see Interface Catalog and Deprecations pages).
- deprecations: deprecated operations advertise `Deprecation: true`; `Sunset: <date>` when scheduled; `Link: rel="deprecation"` points to the doc.
- operationId: snake_case with `_doc` suffix (enforced by Spectral; code‑generated OpenAPI is linted in CI).

## Models

`GET /models/blob/{sha256}`

- Returns the content‑addressed model blob stored under CAS by hex SHA‑256.
- Caching: strong validators with `ETag: "{sha256}"` and `Last-Modified`.
- Clients can send `If-None-Match` to receive `304 Not Modified`.
- Supports `Range: bytes=...` for partial content; returns `206` with `Content-Range`.
- Cache policy: `Cache-Control: public, max-age=31536000, immutable` (digest‑addressed).
 - See also: [HTTP Caching Semantics](../snippets/http_caching_semantics.md)

Examples

```bash
# Full download
curl -SsfLO "http://127.0.0.1:8090/models/blob/0123abcd..."

# Conditional
curl -I -H 'If-None-Match: "0123abcd..."' \
  "http://127.0.0.1:8090/models/blob/0123abcd..."

# Partial
curl -sS -H 'Range: bytes=0-1048575' \
  -o part.bin "http://127.0.0.1:8090/models/blob/0123abcd..."
```
- Concurrency (admin):
  - `POST /admin/models/concurrency` — Set max concurrency at runtime; response includes `pending_shrink` when non‑blocking shrink leaves a remainder.
  - `GET  /admin/models/concurrency` — Snapshot `{ configured_max, available_permits, held_permits, hard_cap, pending_shrink? }`.
  - `GET  /admin/models/jobs` — Active jobs + inflight hashes; includes a concurrency snapshot for context.
Egress
- `GET /state/egress` — recent egress ledger rows `{ id, time, decision, reason?, dest_host?, dest_port?, protocol?, bytes_in?, bytes_out?, corr_id?, proj?, posture }`
- `GET /state/egress/settings` — effective egress posture and toggles
- `POST /egress/settings` — update toggles and persist to config (admin‑gated)
- `POST /egress/preview` — dry‑run URL+method against policy, allowlist, and guards `{ allow, reason?, host, port, protocol }`

Example — `GET /state/egress`
```json
{
  "items": [
    {
      "id": 101,
      "time": "2025-09-15T12:34:56.789Z",
      "decision": "allow",
      "reason": "http",
      "dest_host": "api.github.com",
      "dest_port": 443,
      "protocol": "https",
      "bytes_in": 32768,
      "bytes_out": 0,
      "corr_id": "act_123",
      "proj": "proj-demo",
      "posture": "standard"
    }
  ]
}
```

SSE
- `GET /events?prefix=egress.` — stream `egress.ledger.appended` events as they occur
- Envelope payload example:
  ```json
  {
    "time": "2025-09-15T12:00:00Z",
    "kind": "egress.ledger.appended",
    "payload": {
      "id": 42,
      "decision": "allow",
      "dest_host": "api.github.com",
      "dest_port": 443,
      "protocol": "https",
      "bytes_in": 12345,
      "corr_id": "act_123",
      "proj": "proj-demo",
      "posture": "standard"
    }
  }
  ```
