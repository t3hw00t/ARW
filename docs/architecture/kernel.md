---
title: Kernel (SQLite Journal + CAS)
---

# Kernel (SQLite Journal + CAS)

The Kernel provides a single append‑only event journal (SQLite/WAL) and content‑addressed storage (CAS) co‑located in the state directory. All state views derive from the journal.

Updated: 2025-09-15
Type: Explanation

## Goals
- Single source of truth for events and actions
- Durable replay across restarts
- Portable artifacts via CAS (sha256)
- Foundation for `/actions`, `/events`, `/state` API

## Schema (initial)
- `events(id INTEGER PRIMARY KEY, time TEXT, kind TEXT, actor TEXT NULL, proj TEXT NULL, corr_id TEXT NULL, payload TEXT)`
- `artifacts(sha256 TEXT PRIMARY KEY, mime TEXT, bytes BLOB, meta TEXT)`
- `actions(id TEXT PRIMARY KEY, kind TEXT, input TEXT, policy_ctx TEXT NULL, idem_key TEXT NULL, state TEXT, created TEXT, updated TEXT)`

## API surface (incremental)
- `/triad/events?replay=N` — SSE with optional DB‑backed replay (experimental)
- `/actions` — idempotent action submission (todo)
- `/state/:view` — views derived from SQL (todo)

## Integration
- The in‑process Bus dual‑writes to the Kernel (subscribe + append). This preserves current interactive behavior (SSE) while enabling durable replay.

