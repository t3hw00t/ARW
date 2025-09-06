Spec artifacts (generated) — OpenAPI / AsyncAPI / MCP
Updated: 2025-09-06.

Artifacts:

openapi.yaml — HTTP/WS surfaces (OpenAPI 3.1)

asyncapi.yaml — Event streams (AsyncAPI 2.x)

mcp-tools.json — Catalog of MCP tools available at runtime

schemas/*.json — JSON Schemas (2020-12) for models & operations

How to regenerate (dev):

Build and run the local service with docgen enabled:
cargo run -p arw-docgen --bin arw-docgen

The tool discovers registered operations/events via registries and writes the files here.

CI regenerates these on each PR and pushes to the artifact store; PRs fail on schema breakage.

Do not edit generated files by hand. Change the annotated tool code or event types instead.