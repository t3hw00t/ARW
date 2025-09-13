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
