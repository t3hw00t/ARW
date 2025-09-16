---
title: Spec Endpoints
---

# Spec Endpoints
Updated: 2025-09-16
Type: Reference

The service exposes generated specs under `/spec/*` to help tools and dashboards discover contracts.

Endpoints (unified server)
- `GET /spec/openapi.yaml` — OpenAPI for HTTP endpoints
- `GET /spec/asyncapi.yaml` — AsyncAPI for event topics (bus/SSE)
- `GET /spec/mcp-tools.json` — MCP tools descriptor
- `GET /spec/index.json` — JSON index listing available spec artifacts and JSON Schemas
- `GET /spec/schemas/{file}` — serve individual JSON Schemas
- `GET /catalog/index` — Interface catalog YAML (from `interfaces/index.yaml`)
- `GET /catalog/health` — JSON health summary for spec artifacts

Example: `/catalog/health`
```
{
  "items": [
    {"name":"openapi.yaml","content_type":"application/yaml","path":"spec/openapi.yaml","exists":true,"size":51762},
    {"name":"asyncapi.yaml","content_type":"application/yaml","path":"spec/asyncapi.yaml","exists":true,"size":7932},
    {"name":"mcp-tools.json","content_type":"application/json","path":"spec/mcp-tools.json","exists":true,"size":315}
  ]
}
```

Notes
- Paths are relative to the service’s current working directory; sizes are bytes.
- If a file is missing, `exists` is false and size is 0.
- Override locations with `ARW_SPEC_DIR` (for spec files) and `ARW_INTERFACES_DIR` (for the interface catalog).

Related
- [CLI Guide](../guide/cli.md)
- API Reference (API_AND_SCHEMA.md)
