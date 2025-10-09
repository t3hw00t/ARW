---
title: Agent Recipes
---

# Agent Recipes
Updated: 2025-10-09
Type: How‑to

One‑click bundles that combine prompts, tools, guardrails, and minimal UI into a signed, human‑readable manifest. Install by dropping a folder into `${ARW_STATE_DIR:-state}/recipes/` and launching from the Gallery. The unified server creates this directory on first run; point `ARW_STATE_DIR` elsewhere if you keep state on another volume.

Manifest schema
- JSON Schema at [spec/schemas/recipe_manifest.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/recipe_manifest.json) (versioned)
- Required: id, name, version, model preference, permissions, prompts, tools
- Optional: workflows (steps), ui (form_from_schema, review_required), notes, tags

Example (YAML)
```
id: paperwork-helper
name: Paperwork Helper
version: 1.0.0
model:
  preferred: "local:llama-3.1-8B-instruct"
  fallback: "remote:gpt-4o-mini"
permissions:
  file.read: ask
  file.write: ask
  net.http: never
  shell.exec: never
ui:
  form_from_schema: true
  review_required: true
prompts:
  system: |
    You assist with form-filling. Never submit anything. Show a diff for every change.
tools:
  - id: ocr_pdf
  - id: fill_pdf_form
  - id: summarize_text
workflows:
  - step: "Extract text"
    tool: ocr_pdf
  - step: "Propose filled form"
    tool: fill_pdf_form
  - step: "Summarize differences for review"
    tool: summarize_text
```

Permissions
- Modes: `ask | allow | never` with optional TTL leases
- Capabilities: `fs(read|write)`, `net(http)`, `shell(exec)`, `mic`, `cam`, `gpu`, `sandbox:<kind>`
- Decisions are emitted as `Policy.*` events and rendered inline in the sidecar

Distribution
- Local folders inside the user’s state dir
- Optional signed recipe index (static JSON) for curated catalogs (libraries, schools, unions)

UX notes
- Auto‑form tool params from ARW tool JSON Schemas; validate client‑side before dispatch
- Intent preview: “This will read 3 files and send 1 email”
- Activity renders in the Episodes timeline; snapshot manifests for reproducibility

See also: UI Architecture, Policy, Context Recipes.
