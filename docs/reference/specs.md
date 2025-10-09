---
title: Spec Endpoints
---

# Spec Endpoints
Updated: 2025-10-09
Type: Reference

The service exposes generated specs under `/spec/*` to help tools and dashboards discover contracts.

Endpoints (unified server)
- `GET /spec/openapi.yaml` — OpenAPI for HTTP endpoints
- `GET /spec/asyncapi.yaml` — AsyncAPI for event topics (bus/SSE)
- `GET /spec/mcp-tools.json` — MCP tools descriptor
- `GET /spec/index.json` — JSON index listing available spec artifacts and JSON Schemas
- `GET /spec/health` — JSON stats for spec artifacts (exists/size/modified)
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
- Spec health also includes `modified_ms` (Unix epoch ms) when available and a summary of JSON schemas (`schemas.{exists,count,files}`).
- Override locations with `ARW_SPEC_DIR` (for spec files) and `ARW_INTERFACES_DIR` (for the interface catalog).
- Featured schemas: `modular_agent_message.json` (typed specialist agent envelopes with chat/recall/compression/interpretation/validation/tool/orchestrator payloads) and `modular_tool_invocation.json` (tool brokerage contracts) underpin the modular cognitive stack rollout. Supporting endpoints include `GET /state/memory/recent` (rich summary snapshot), `GET /state/memory/modular` (lightweight modular review feed), and `GET /state/memory/lane/{lane}` for lane-scoped snapshots.

Related
- [CLI Guide](../guide/cli.md)
- API Reference (API_AND_SCHEMA.md)
- [Runtime manifest schema](../architecture/managed_runtime_supervisor.md#current-implementation-snapshot) (served as `spec/schemas/runtime_manifest.json`)
