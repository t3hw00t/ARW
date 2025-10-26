---
title: Adapter SDK
---

# Adapter SDK
Updated: 2025-10-26
Type: How‑to

This guide shows how to build and validate runtime adapters for ARW using the `arw-runtime-adapter` SDK and the adapter manifest schema.

- SDK Crate: `crates/arw-runtime-adapter` (Rust)
- Manifest Schema: `spec/schemas/runtime_adapter_manifest.schema.json` (also published under `docs/spec/schemas/` for editors)
- CLI Helpers: `arw-cli adapters *` (validate, init, schema)

## Quick Start

- Create a manifest (JSON):
  - `cargo run -p arw-cli -- adapters init --out adapters/my.adapter.json --id my.adapter --name "My Adapter"`
  - Open `adapters/my.adapter.json` in VS Code; it will auto‑resolve the schema for validation.

- Validate a manifest:
  - `cargo run -p arw-cli -- adapters validate --manifest adapters/my.adapter.json`
  - Strict warnings: add `--strict-warnings` (CI uses strict).

- Generate/update the schema:
  - `cargo run -p arw-cli -- adapters schema --out spec/schemas/runtime_adapter_manifest.schema.json`
  - Copies are published to `docs/spec/schemas/` for editor tooling.

## Implementing an Adapter (Rust)

Adapters implement `arw_runtime::RuntimeAdapter` and expose a stable entrypoint matching the `entrypoint.symbol` in the manifest.

Example skeleton:

```rust
use arw_runtime::{
  AdapterError, PrepareContext, PreparedRuntime, RuntimeAdapter,
  RuntimeAdapterMetadata, RuntimeHandle, RuntimeHealthReport,
  RuntimeModality, RuntimeAccelerator,
};

pub struct MyAdapter;

#[async_trait::async_trait]
impl RuntimeAdapter for MyAdapter {
  fn id(&self) -> &'static str { "my.adapter" }

  fn metadata(&self) -> RuntimeAdapterMetadata {
    RuntimeAdapterMetadata {
      modalities: vec![RuntimeModality::Text],
      default_accelerator: Some(RuntimeAccelerator::Cpu),
      ..Default::default()
    }
  }

  async fn prepare(&self, ctx: PrepareContext<'_>) -> Result<PreparedRuntime, AdapterError> {
    // Build a process command from descriptor/profile/tags
    Ok(PreparedRuntime {
      command: "my-runtime-binary".to_string(),
      args: vec!["--port".into(), "8000".into()],
      runtime_id: None,
    })
  }

  async fn launch(&self, prepared: PreparedRuntime) -> Result<RuntimeHandle, AdapterError> {
    // Spawn process and return handle (pid optional)
    Ok(RuntimeHandle { id: "runtime-1".into(), pid: None })
  }

  async fn shutdown(&self, _handle: RuntimeHandle) -> Result<(), AdapterError> {
    Ok(())
  }

  async fn health(&self, _handle: &RuntimeHandle) -> Result<RuntimeHealthReport, AdapterError> {
    // Map your runtime health into RuntimeStatus
    Ok(RuntimeHealthReport { status: arw_runtime::RuntimeStatus::ready("runtime-1") })
  }
}

// Manifest entrypoint: match entrypoint.symbol in your manifest
#[no_mangle]
pub extern "C" fn create_adapter() -> arw_runtime::BoxedAdapter {
  Box::new(MyAdapter)
}
```

Pair this with a manifest that points `entrypoint.crate_name` to your crate and `entrypoint.symbol` to `create_adapter`.

## Manifest Fields (Overview)

- `id` (required): stable id `[A-Za-z0-9._-]+`.
- `version` (required): semver string, e.g., `0.1.0`.
- `modalities` (required): e.g., `["text"]`.
- `entrypoint` (required): `{ crate_name, symbol, kind? }`.
- `resources`: accelerator, recommended memory/threads, network needs.
- `consent`: short, operator‑facing capability summary (+ optional URL, capabilities).
- `metrics`: optional adapter metrics (Prometheus‑style names).
- `health`: poll/grace periods (+ optional `status_endpoint`).

See: `docs/reference/adapter_manifest.md` for full schema details.

## Task Shortcuts (Repo Root)

- Just
  - `just adapters-validate manifest=adapters/demo.adapter.json` – validate one manifest
  - `just adapters-lint` – lint all manifests under `adapters/`
  - `just adapters-schema` – regenerate + copy schema for docs

- Mise
  - `mise run adapters:validate MANIFEST=adapters/demo.adapter.json`
  - `mise run adapters:lint`
  - `mise run adapters:schema`

## CI

- The CI lints manifests under `adapters/` with strict warnings when relevant files change and runs a lightweight smoke.
  - Lint: validates manifests (normal + strict).
  - Smoke: `scripts/adapter_smoke.sh` validates all manifests and can optionally probe health endpoints.
  - JSON report: set `ADAPTER_SMOKE_OUT=/tmp/adapter-smoke.json` to emit a structured report (CI uploads as an artifact).

### Smoke Harness (local)

- Validate + advisories only:
  - `bash scripts/adapter_smoke.sh`

- Include best‑effort health probes (JSON manifests only):
  - `ADAPTER_SMOKE_HEALTH=1 bash scripts/adapter_smoke.sh`

- Emit a JSON report:
  - `ADAPTER_SMOKE_OUT=dist/adapter-smoke.json bash scripts/adapter_smoke.sh`

The JSON report includes per‑manifest status and advisories (e.g., missing consent summary or description) that are non‑fatal recommendations in addition to SDK validation.
