# Interface Catalog
Updated: 2025-09-16
Type: Reference

This repo keeps a single, machine‑readable catalog of every interface (HTTP, events/SSE, and MCP tools) under `interfaces/`. Each interface has a small descriptor with ownership, lifecycle, and a pointer to the full spec.

- Catalog index: `interfaces/index.yaml` (also exposed at `/catalog/index`)
- HTTP (OpenAPI 3.1): `spec/openapi.yaml`
- Events (AsyncAPI): `spec/asyncapi.yaml`
- MCP tools: `spec/mcp-tools.json`

Descriptors (examples):

- `interfaces/http/arw-server/descriptor.yaml` → ARW unified server API (canonical), points to `spec/openapi.yaml`
- `interfaces/http/arw-svc/descriptor.yaml` → ARW service API (legacy), points to `spec/openapi.yaml`
- `interfaces/events/arw/descriptor.yaml` → ARW event channels (SSE), points to `spec/asyncapi.yaml`
- `interfaces/tools/arw/descriptor.yaml` → MCP toolset, points to `spec/mcp-tools.json`

Quality gates:

- Spectral lint rules: `quality/openapi-spectral.yaml`
- CI verifies: lint, diffs (breaking changes), mock smoke test, and descriptor date hygiene
- Deprecation/Sunset: when operations are marked `deprecated: true` in OpenAPI, runtime emits `Deprecation: true` and (if set in the descriptor) `Sunset: <iso8601>`, plus `Link: <docs>; rel="deprecation"`

Update workflow:

1. Edit interface specs in `spec/` (or regenerate via `OPENAPI_OUT` for the service).
2. Update relevant descriptors under `interfaces/` (owner, lifecycle, docs).
3. Regenerate the catalog index: `python scripts/interfaces_generate_index.py`.
4. Commit changes; CI will lint, diff, and check the index is up‑to‑date.

Runtime endpoints:

- `/catalog/index` → serves the catalog index YAML (unified server)
- `/catalog/health` → JSON presence/size summary for spec artifacts (unified server)
- `/spec/*` → serves raw spec files (OpenAPI/AsyncAPI/MCP) and `/spec/index.json`
- `/events` (SSE) → JSON envelope by default; `ce-structured` mode available via env
