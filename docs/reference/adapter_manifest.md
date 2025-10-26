---
title: Adapter Manifest
---

# Adapter Manifest

Updated: 2025-10-26
Type: Reference

Adapter manifests describe third‑party runtime adapters for the Managed Runtime Supervisor. Use them to declare modalities, resources, consent, and health behavior so the supervisor can validate and launch adapters safely.

Quick validation
- Human readable: `cargo run -p arw-cli -- adapters validate --manifest examples/adapters/demo.adapter.json`
- JSON output: `cargo run -p arw-cli -- adapters validate --manifest <path> --json --pretty`
- Strict mode (warnings fail): add `--strict-warnings`
- Justfile helper: `just adapters-validate manifest=<path> json=true pretty=true`

Supported formats
- JSON or TOML; file extension selects the parser. Unknown extensions try JSON then TOML.
- Examples: `examples/adapters/demo.adapter.json` and `examples/adapters/demo.adapter.toml`.

Required fields
- `id` (string): ASCII letters/digits `[-_.]` only.
- `version` (semver): e.g., `0.1.0`.
- `entrypoint.crate_name` (string)
- `entrypoint.symbol` (string)
- `modalities` (array): at least one of `text`, `audio`, `vision`.

Common fields
- `name`, `description`
- `tags` (array of strings): lowercase, 1–32 chars, `[a-z0-9_-]` recommended.
- `resources`:
  - `accelerator`: `cpu`, `gpu_cuda`, `gpu_rocm`, `gpu_metal`, `gpu_vulkan`, `npu_directml`, `npu_coreml`, `npu_other`, `other`
  - `recommended_memory_mb` (>0)
  - `recommended_cpu_threads` (>=1)
  - `requires_network` (bool)
- `consent`:
  - `summary` (string)
  - `details_url` (http/https)
  - `capabilities` (e.g., `egress` when `requires_network=true`)
- `metrics` (name/description/unit): names follow `^[a-zA-Z_:][a-zA-Z0-9_:]*$`
- `health`:
  - `poll_interval_ms` (default 5000; warns below 500)
  - `grace_period_ms` (should be ≥ `poll_interval_ms`)
  - `status_endpoint` (optional)

Validator behavior
- Errors: block usage (missing `modalities`, invalid `id` or `version`, zero memory/threads, empty metric name).
- Warnings: hygiene/safety hints (duplicate tags/metrics, low memory hint, URL scheme, network consent, fast polling, short grace period).

Examples
- Clean: `examples/adapters/demo.adapter.json` (mirrored under `adapters/` for CI)
- Warnings: `examples/adapters/warn-demo.adapter.json` (mirrored under `adapters/`)

Stable JSON Schema
- Hosted: https://t3hw00t.github.io/ARW/spec/schemas/runtime_adapter_manifest.schema.json
- File: `spec/schemas/runtime_adapter_manifest.schema.json` (also copied into `docs/spec/schemas/` for site publishing)
- Tip: Set `$schema` and `$id` in your manifest for IDE validation:
  - `$schema`: `http://json-schema.org/draft-07/schema#`
  - `$id`: `https://t3hw00t.github.io/ARW/spec/schemas/runtime_adapter_manifest.schema.json`

Project manifests directory
- Place real manifests under `adapters/` to be linted in CI.
- Local lint all: `bash scripts/lint_adapters.sh` (set `ADAPTERS_LINT_STRICT_WARNINGS=1` to fail on warnings).
