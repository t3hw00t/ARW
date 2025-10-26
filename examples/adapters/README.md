# Adapter Examples

Updated: 2025-10-26
Type: Reference

This folder contains example adapter manifests for the Managed Runtime Supervisor.

Files
- `demo.adapter.json` — clean example; validates with no warnings.
- `warn-demo.adapter.json` — demonstrates non‑fatal warnings (tag casing/dup, low memory hint, missing `egress` capability when `requires_network=true`, duplicate metric).

See also
- How‑to: `docs/guide/adapters_validate.md`
- Reference: `docs/reference/adapter_manifest.md`
- JSON Schema (hosted): https://t3hw00t.github.io/ARW/spec/schemas/runtime_adapter_manifest.schema.json

