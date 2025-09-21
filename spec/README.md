Agent Hub (ARW) API and Schema Artifacts

This folder hosts generated specifications produced by the doc pipeline (`scripts/docgen.sh`):
- spec/openapi.yaml — HTTP API surface (OpenAPI 3.1)
- spec/asyncapi.yaml — Event streams (AsyncAPI 2.x)
- spec/mcp-tools.json — MCP tool catalog
- spec/schemas/*.json — JSON Schemas for operations and models

Artifacts are sourced from runtime metadata (via `arw-cli`) and event topics extracted by `scripts/gen_asyncapi.py`, keeping code and specs aligned.

In CI, compatibility guards will validate schema evolution and publish artifacts.
