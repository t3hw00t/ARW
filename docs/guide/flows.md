---
title: Flows (Logic Units)
---

# Flows (Logic Units)

A minimal “Flows” page is available in debug builds to compose and apply Logic Unit patches visually. It emits JSON patches to the existing Logic Units API.

Updated: 2025-09-20
Type: How‑to

## Open the Page

- Set `ARW_DEBUG=1`
- Unified server (default local dev port `http://127.0.0.1:8091`): open `http://127.0.0.1:8091/ui/flows` (or `/admin/ui/flows` with an admin token).
- The legacy 8090 bridge has been retired; run the unified server instead.

!!! note "Legacy debug UI"
    The `/admin/ui/flows` surface ships with `arw-server` (enable `ARW_DEBUG=1`). The underlying APIs continue to live under `/logic-units/*`.

## What It Does

- Lets you set a Unit ID and optional scope and paste a JSON array of patches
- Dry‑run or Apply via `POST /logic-units/apply`
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

You can call the shipped Logic Units endpoints directly from clients:

- `POST /logic-units/apply` — apply patches
- `POST /logic-units/install` — register a unit manifest
- `POST /logic-units/revert` — revert by snapshot id

See also: Guide → Logic Units Library; Architecture → Config Patch Engine.
