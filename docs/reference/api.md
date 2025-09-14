# API Reference

Microsummary: Public endpoints, admin surfaces, specs, and eventing. Stable/experimental flags are surfaced in specs; deprecations emit standard headers.

- Specs in repo: `spec/openapi.yaml`, `spec/asyncapi.yaml`, `spec/mcp-tools.json`
- Specs at runtime: `GET /spec/openapi.yaml`, `GET /spec/asyncapi.yaml`, `GET /spec/mcp-tools.json`
- Catalog: `GET /catalog/index` (YAML) and `GET /catalog/health` (JSON)
- Auth: Local‑only by default; for admin endpoints set `ARW_ADMIN_TOKEN` and send `Authorization: Bearer <token>` or `X-ARW-Admin`.

Endpoints (selected)
- `GET /healthz`: service health, returns `ok` when ready.
- `GET /debug`: Debug UI (when `ARW_DEBUG=1`).
- `GET /admin/events`: SSE for live updates; supports `?replay=N` and `Last-Event-ID`.
- `GET /state/*`: read‑models (observations, beliefs, world, intents, actions, episodes, self/{agent}).

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
