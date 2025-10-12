---
title: Agent Recipes
---

# Agent Recipes
Updated: 2025-10-12
Type: How‑to

One‑click bundles that combine prompts, tools, guardrails, and minimal UI into a signed, human‑readable manifest. Install by dropping a folder into `${ARW_STATE_DIR:-state}/recipes/` and launching from the Gallery. The unified server creates this directory on first run; point `ARW_STATE_DIR` elsewhere if you keep state on another volume.

Manifest schema
- JSON Schema at [spec/schemas/recipe_manifest.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/recipe_manifest.json) (versioned)
- Required: id, name, version, model preference, permissions, prompts, tools
  - `id`: lower-case slug (`[a-z0-9._-]`, 1–64 chars)
  - `version`: semantic version (`1.2.3`, with optional prerelease/build metadata)
  - `permissions`: at least one capability key (lower-case `scope` or `scope.action`); value is `ask`, `allow`, `never`, or an object with `mode` + optional `ttl_secs`
  - `tools`: unique tool ids (match installed tool registry); optional `params` object per tool
- Optional: workflows (unique steps referencing declared tools), `ui` (form_from_schema, review_required), `tags` (unique strings), `notes` (non-empty string), `prompts.user_template`

Example (YAML)
```yaml
--8<-- "examples/recipes/paperwork-helper.yaml"
```

Permissions
- Modes: `ask | allow | never` with optional TTL leases
- Capabilities: `file.read`, `file.write`, `net.http`, `shell.exec`, `mic`, `cam`, `gpu`, `sandbox:<kind>` (follow the lower-case scope/verb pattern)
- Decisions are emitted as `Policy.*` events and rendered inline in the sidecar

Additional samples
- [examples/recipes/incident-reviewer.yaml](https://github.com/t3hw00t/ARW/blob/main/examples/recipes/incident-reviewer.yaml) — SRE post-incident workflow with TTL-limited file access and multi-step outline generation.
- [examples/recipes/research-digest.yaml](https://github.com/t3hw00t/ARW/blob/main/examples/recipes/research-digest.yaml) — Weekly research digest with feed aggregation, clustering, and project-specific delivery cues.
- [examples/recipes/web-browsing.yaml](https://github.com/t3hw00t/ARW/blob/main/examples/recipes/web-browsing.yaml) — Out-of-the-box browsing assistant that fetches pages with `http.fetch` and summarizes results.

CLI helpers
- `arw-cli recipes inspect <path>` validates a manifest (file or folder) and prints a readable summary.
- `arw-cli recipes install <path>` copies a validated manifest into `${ARW_STATE_DIR}/recipes/<id>` (pass `--force` to overwrite).
- `arw-cli recipes list` shows installed recipes; add `--json` for machine-readable output.

Distribution
- Local folders inside the user’s state dir
- Optional signed recipe index (static JSON) for curated catalogs (libraries, schools, unions)

UX notes
- Auto‑form tool params from ARW tool JSON Schemas; validate client‑side before dispatch
- Intent preview: “This will read 3 files and send 1 email”
- Activity renders in the Episodes timeline; snapshot manifests for reproducibility

See also: UI Architecture, Policy, Context Recipes.
