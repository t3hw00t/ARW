Agent Hub (ARW) API and Schema Artifacts

This folder hosts generated specifications produced by the doc pipeline (`scripts/docgen.sh`) and by running the unified server with `OPENAPI_OUT`:
- spec/openapi.yaml — HTTP API surface (OpenAPI 3.1)
- spec/asyncapi.yaml — Event streams (AsyncAPI 2.x)
- spec/mcp-tools.json — MCP tool catalog
- spec/schemas/*.json — JSON Schemas for operations and models

Artifacts are sourced from runtime metadata (via `arw-cli`) and event topics extracted by `scripts/gen_asyncapi.py`, keeping code and specs aligned. The HTTP OpenAPI document is emitted directly from `apps/arw-server` annotations; regenerate locally with:

```
OPENAPI_OUT=spec/openapi.yaml cargo run --no-default-features -p arw-server
python3 scripts/ensure_openapi_descriptions.py
python3 scripts/generate_openapi_json.py
```

In CI, compatibility guards will validate schema evolution and publish artifacts.
