# Schema Reference (JSON)
Updated: 2025-10-26
Type: Reference

Microsummary: First‑class JSON Schemas for recipes, tools, and runtime artifacts with links for tooling. Stable anchors.

Locations
- Directory: [spec/schemas/](https://github.com/t3hw00t/ARW/tree/main/spec/schemas) (JSON)

Highlighted schemas
- Recipe manifest: [spec/schemas/recipe_manifest.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/recipe_manifest.json) — installable strategy packs (prompts, tools, permissions, workflows).
- Mini-agent catalog entry: [spec/schemas/mini_agent.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/mini_agent.json) — curated Training Park entries with presets, requirements, and documentation.
- Model manifest: [spec/schemas/model_manifest.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/model_manifest.json) - installed model metadata (CAS filename/hash/bytes/provider).
- Runtime adapter manifest: [spec/schemas/runtime_adapter_manifest.schema.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/runtime_adapter_manifest.schema.json) - third-party runtime adapters for the Managed Runtime Supervisor.
  - Hosted: https://t3hw00t.github.io/ARW/spec/schemas/runtime_adapter_manifest.schema.json
- Logic unit manifest: [spec/schemas/logic_unit_manifest.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/logic_unit_manifest.json) - config-first strategy packs with patches and permission leases.
- Egress ledger: [spec/schemas/egress_ledger.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/egress_ledger.json) — append-only egress records (allow/deny, posture, attribution).
- Self-model: [spec/schemas/self_model.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/self_model.json) — compact agent self-model (capabilities, competence, calibration, costs).
- Gating policy config: [reference/gating_config.schema.json](gating_config.schema.json) — immutable denies and conditional contracts loaded at startup.

Notes
- Schemas include `$id` and allow `$comment` for license (SPDX) and guidance fields.
- Use these files directly in tooling; the Debug UI links to relevant Reference pages.
