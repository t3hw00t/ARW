ARW API and Schema Artifacts

This folder hosts generated specifications produced by future tooling (arw-docgen):
- spec/openapi.yaml — HTTP API surface (OpenAPI 3.1)
- spec/asyncapi.yaml — Event streams (AsyncAPI 2.x)
- spec/mcp-tools.json — MCP tool catalog
- spec/schemas/*.json — JSON Schemas for operations and models

Artifacts are generated from Rust tool/operation declarations (#[arw_tool]) to ensure a single source of truth.

In CI, compatibility guards will validate schema evolution and publish artifacts.

