---
title: Flows (Logic Units)
---

# Flows (Logic Units)

A minimal “Flows” page is available in debug builds to compose and apply Logic Unit patches visually. It emits JSON patches to the existing Logic Units API.

Updated: 2025-09-15
Type: How‑to

## Open the Page

- Set `ARW_DEBUG=1`
- Open: `http://127.0.0.1:8090/ui/flows` (or `/admin/ui/flows` with an admin token). In the unified server, planned endpoints live under `/logic-units/*`.

## What It Does

- Lets you set a Unit ID and optional scope and paste a JSON array of patches
- Dry‑run or Apply via `POST /admin/logic-units/apply`
- Shows the result payload for quick iteration

## Patch Example

```json
[
  {
    "target": "governor.hints",
    "op": "merge",
    "value": { "mode": "verified", "retrieval_k": 20, "mmr_lambda": 0.3 }
  }
]
```

This updates planner/governor hints to favor a verified mode with stronger retrieval.

## Programmatic Use

You can call Logic Units endpoints directly from clients:

- `POST /admin/logic-units/apply` — apply patches
- `POST /admin/logic-units/install` — register a unit manifest
- `POST /admin/logic-units/revert` — revert by snapshot id

See also: Guide → Logic Units Library; Architecture → Config Patch Engine.
