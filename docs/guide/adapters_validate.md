---
title: Validate Adapter Manifests
---

# Validate Adapter Manifests

Updated: 2025-10-26
Type: How-to

This guide shows quick ways to validate adapter manifests (JSON/TOML) for third‑party runtimes.

One-off validation
- Human summary: `cargo run -p arw-cli -- adapters validate --manifest <path>`
- JSON output: `cargo run -p arw-cli -- adapters validate --manifest <path> --json --pretty`
- Strict mode (warnings fail): add `--strict-warnings`

Justfile helper
- `just adapters-validate manifest=<path> strict=true`

Validate all manifests
- Place manifests under `adapters/` in the repo.
- Run: `bash scripts/lint_adapters.sh`
- Fail on warnings: `ADAPTERS_LINT_STRICT_WARNINGS=1 bash scripts/lint_adapters.sh`
- Include in verify: set `ARW_VERIFY_INCLUDE_ADAPTERS=1` and run `bash scripts/dev.sh verify`.

CI integration
- GitHub Actions workflow `Adapters Lint` validates manifests on every PR/push.
- PRs: runs both non‑strict and strict; lints only changed manifests under `adapters/`.
- Pushes: runs strict‑only; lints the entire `adapters/` directory.
 - The job publishes a Markdown table summary and file-level annotations for warnings/errors.

Schema for IDEs
- JSON Schema lives at `spec/schemas/runtime_adapter_manifest.schema.json`.
- Generate/update: `cargo run -p arw-cli -- adapters schema --out spec/schemas/runtime_adapter_manifest.schema.json`

Pre-commit hook (optional)
- Install: `pip install pre-commit && pre-commit install`
- Config: `.pre-commit-config.yaml` includes an `adapters-lint` hook that runs on changes in `adapters/`.

Examples
- Clean: `examples/adapters/demo.adapter.json` (mirrored in `adapters/`)
- Warnings: `examples/adapters/warn-demo.adapter.json` (mirrored in `adapters/`)
- TOML: `examples/adapters/demo.adapter.toml`

Scaffold a manifest
- Create a starter file: `cargo run -p arw-cli -- adapters init --out adapters/my.adapter.json --id my.org.adapter`
- Use TOML: add `--format toml`
