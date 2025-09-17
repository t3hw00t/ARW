# AI Assistants Index
Updated: 2025-09-16
Type: Reference

Microsummary: Map of repo for AI/code models: purpose, key files, how to run, limits. Stable entrypoint for tooling.

- Purpose: unified local-first agents service (`arw-server` in `apps/arw-server/`) plus tooling surfaces; legacy `arw-svc` stays available for the classic debug UI during the restructure.
- Quick run: follow `docs/guide/quickstart.md` for the unified flow; add `--legacy` only when you need the old debug UI bridge.
- Key files:
  - Primary service: `apps/arw-server/` (Rust unified server). Legacy debug stack: `apps/arw-svc/` (maintenance mode).
  - CLI: `apps/arw-cli/`; desktop launcher: `apps/arw-launcher/` (currently defaults to legacy until porting completes).
  - Docs: `docs/` (MkDocs Material); site config: `mkdocs.yml`.
  - Schemas: `spec/schemas/`; OpenAPI preview: `docs/static/openapi.json`.
- Limits: default‑deny for write/shell/network; assume no internet; prefer explicit permissions.
