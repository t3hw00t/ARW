# Repo Map (for assistants)
Updated: 2025-10-11
Type: Reference

Microsummary: High‑level map of this workspace for fast orientation. Stable headings for retrieval.

Need a workflow primer? Start with [Agent Onboarding](AGENT_ONBOARDING.md) before diving into the sections below.

Top‑level
- `crates/` — Rust workspace crates (core services, libraries).
- `apps/` — App surfaces (service/CLI/launchers).
- `docs/` — MkDocs content (guides, architecture, reference).
- `spec/` — Machine‑readable specs (JSON Schemas, etc.).
- `examples/` — Minimal, runnable usage examples.
- `scripts/` — Setup/start helpers (PowerShell/Bash).
- `.mise.toml` — Pinned toolchain + task shortcuts (`mise install`, `mise run verify`, etc.).
- `configs/`, `deploy/` — Packaging & deployment (Compose/Helm/etc.).
- `sandbox/` — Scratch area for experiments.

Key concepts from README
- Local-first service with Debug UI at `/admin/debug`, state at `/state/*`, SSE events.
- “Commons Kit” recipes and tool schemas; recipe manifest schema in `spec/schemas`.
- Tooling: `scripts/dev.{sh,ps1}` for guardrails, `scripts/bootstrap_docs.sh` (or `mise run bootstrap:docs`) for MkDocs dependencies in `.venv/docs`, `scripts/dev.{sh,ps1} docs-cache` / `scripts/build_docs_wheels.sh` (`mise run docs:cache:build`) for offline wheel bundles.

(Use this map as the first chunk for retrieval; keep headings stable.)
