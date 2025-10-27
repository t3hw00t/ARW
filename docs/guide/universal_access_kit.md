---
title: Universal Access Kit
---

# Universal Access Kit

Updated: 2025-10-27
Type: How-to

The Universal Access Kit bundles eco-friendly defaults, quickstart docs, and a starter persona so you can bring ARW online on low-spec or offline machines. When build tools are present, it also includes an offline MkDocs site and the mini dashboard binary.

What’s included
- docs/: selected quickstart and offline guides (Markdown). If MkDocs is installed, a full site/ build is added.
- config/: eco-preset.env, persona_seed.json, kit-notes.md
- bin/: arw-mini-dashboard (optional, if Cargo is available)
- README.html: offline entry page linking to site/ (if present) and docs/ Markdown quickstarts

Build the kit
- Just: `just kit-universal`
- Mise: `mise run access:kit`

Lite build (skip optional extras)
- Just: `just kit-universal-lite`
- Mise: `mise run access:kit:lite`

Validate an existing kit
- Just: `just kit-universal-check`
- Mise: `mise run access:kit:check`

Notes
- If `mkdocs` is available, the kit includes a `site/` folder with the offline docs site.
- If `cargo` is available, the kit includes `bin/arw-mini-dashboard` (tiny read‑model watcher).
- Eco preset values are sourced from `configs/presets/examples.toml`. Explicit env vars always override.
 - Set `ARW_KIT_SKIP_OPTIONAL=1` to skip the optional docs site and binary bundling.

CI
- On kit-related changes, CI assembles and validates the kit and uploads `universal-access-kit.zip` as an artifact for download.

Quick start
1) Source `config/eco-preset.env` before launching the server.
2) Seed a persona: `arw-cli admin persona seed --from ./config/persona_seed.json`.
3) Start the server and verify `/healthz` and `/about`.
4) (Optional) Run the mini dashboard: `./bin/arw-mini-dashboard --base http://127.0.0.1:8091`.
