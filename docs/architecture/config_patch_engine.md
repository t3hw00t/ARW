---
title: Config Plane & Patch Engine
---

# Config Plane & Patch Engine

Purpose
- Atomically apply/revert/validate "Logic Unit" config bundles (and other config deltas) against versioned schemas. Produce a human‑readable diff; fail‑closed on validation; audit any permission widening.

Principles
- Data‑first: patches are JSON Merge/Patch documents; no dynamic code needed for the common path.
- Deterministic: ordered application with pre/post validation against schemas (recipes, policies, flows, tools).
- Audited: emits `LogicUnit.Applied/Reverted` and `Policy.Decision` events when permissions change.

Flow
1) Dry‑run: compute diff and validate; return human‑readable summary (added/changed/removed) and warnings.
2) Apply: atomically update config; emit events; snapshot effective config; offer one‑click rollback.
3) Revert: restore last snapshot; emit `LogicUnit.Reverted`.

Endpoints (planned)
- `POST /patch/dry-run` → `{ diff, warnings }`
- `POST /patch/apply` → apply unit or bundle; emits events
- `POST /patch/revert` → snapshot id or unit id

See also: Logic Units, Permissions & Policies, Replay & Time Travel.

