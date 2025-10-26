# Adapters

Updated: 2025-10-26
Type: Reference

This directory holds runtime adapter manifests that integrate third‑party runtimes with the Managed Runtime Supervisor.

Quick links
- How-to: docs/guide/adapters_validate.md
- Reference: docs/reference/adapter_manifest.md
- JSON Schema (hosted): https://t3hw00t.github.io/ARW/spec/schemas/runtime_adapter_manifest.schema.json

Validate locally
- Single file (human): `cargo run -p arw-cli -- adapters validate --manifest adapters/demo.adapter.json`
- JSON output: add `--json --pretty`
- Strict warnings: add `--strict-warnings`
- Lint all: `bash scripts/lint_adapters.sh`
- Verify integration: `ARW_VERIFY_INCLUDE_ADAPTERS=1 bash scripts/dev.sh verify`

CI
- PRs: non‑strict and strict runs on changed manifests
- Pushes: strict‑only on all manifests in this folder

