---
title: Artifacts & Provenance
---

# Artifact Registry with Provenance

Artifact entity
- Types: file, message, index, report, snapshot, dataset, model-manifest.
- Fields: id, kind, path/ref, size, checksum, created_at, created_by (agent/profile), tags.

Provenance edges
- `{ inputs[] → tool/model/policy/context → artifact }` captured as events and persisted to a graph store.
- Snapshots bundle effective config (prompts, recipes, policies, versions) for deterministic replay.

Built‑ins (planned)
- Reserved events: `artifact.created`, `artifact.linked`, `artifact.deleted`.
- Endpoint sketch: `POST /artifacts/export` for bundles.

UI
- Show “Evidence” links for risky actions (open the provenance graph slice).

See also: Replay & Time Travel, Versioning & Migration, Data Governance.
