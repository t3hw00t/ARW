# AI Assistants Index
Updated: 2025-09-16
Type: Reference

Microsummary: Map of repo for AI/code models: purpose, key files, how to run, limits. Stable entrypoint for tooling.

- Purpose: local‑first agents service (`arw-svc`) with Debug UI, CLI, and recipes.
- Quick run: see `README.md` → Try in 2 Minutes; docs at `/` and `/debug` once running.
- Key files:
  - Service: `apps/arw-svc/` (Rust); CLI: `apps/arw-cli/`; desktop launcher: `apps/arw-launcher/`.
  - Docs: `docs/` (MkDocs Material); site config: `mkdocs.yml`.
  - Schemas: `spec/schemas/`; OpenAPI preview: `docs/static/openapi.json`.
- Limits: default‑deny for write/shell/network; assume no internet; prefer explicit permissions.
