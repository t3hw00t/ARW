# AI Assistants Index
Updated: 2025-09-22
Type: Reference

Microsummary: Map of repo for AI/code models: purpose, key files, how to run, limits. Stable entrypoint for tooling.

- Purpose: unified local-first agents service (`arw-server` in `apps/arw-server/`) plus tooling surfaces.
- Quick run: follow `docs/guide/quickstart.md` for the unified flow; use `scripts/debug.{sh,ps1} --open` with `ARW_DEBUG=1` to open `/admin/debug`.
- Key files:
  - Primary service: `apps/arw-server/` (Rust unified server).
  - CLI: `apps/arw-cli/`; desktop launcher: `apps/arw-launcher/` (drives the unified server by default).
  - Docs: `docs/` (MkDocs Material); site config: `mkdocs.yml`.
  - Schemas: `spec/schemas/`; OpenAPI preview: `docs/static/openapi.json`.
- Limits: default‑deny for write/shell/network; assume no internet; prefer explicit permissions.
