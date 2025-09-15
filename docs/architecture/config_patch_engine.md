---
title: Config Plane & Patch Engine
---

# Config Plane & Patch Engine
Updated: 2025-09-15
Type: Explanation

Purpose
- Atomically apply/revert/validate "Logic Unit" config bundles (and other config deltas) against versioned schemas. Produce a human‑readable diff; fail‑closed on validation; audit any permission widening.

Principles
- Data‑first: patches are JSON Merge/Patch documents; no dynamic code needed for the common path.
- Deterministic: ordered application with pre/post validation against schemas (recipes, policies, flows, tools).
- Audited: emits `logic.unit.applied`/`logic.unit.reverted` and `policy.decision` events when permissions change.

Flow
1) Dry‑run: compute diff and validate; return human‑readable summary (added/changed/removed) and warnings.
2) Apply: atomically update config; emit events; snapshot effective config; offer one‑click rollback.
3) Revert: restore last snapshot; emit `logic.unit.reverted`.

Endpoints (planned)
- `POST /patch/dry-run` → `{ diff, warnings }`
- `POST /patch/apply` → apply unit or bundle; emits events
- `POST /patch/revert` → snapshot id or unit id

See also: Logic Units, Permissions & Policies, Replay & Time Travel.

## Mapped Segments (Schema Map)

The Patch Engine can infer schemas for top‑level segments via a schema map file (default `configs/schema_map.json`, or set `ARW_SCHEMA_MAP`).

Example entries
```json
{
  "recipes": { "schema_ref": "spec/schemas/recipe_manifest.json", "pointer_prefix": "recipes" },
  "policy":  { "schema_ref": "spec/schemas/policy_network_scopes.json", "pointer_prefix": "policy" },
  "egress":  { "schema_ref": "spec/schemas/egress_settings.json", "pointer_prefix": "egress" }
}
```

This enables validating `egress` runtime settings and `policy` network scopes during patch application, with snapshots and rollback.
