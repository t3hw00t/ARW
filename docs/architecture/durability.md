---
title: Durability & Offline
---

# Durability & Offline
Updated: 2025-09-15
Type: Explanation

Goals
- Event journal + periodic snapshots; idempotent tool actions; crash‑safe resume; explicit conflict resolution when offline.

Patterns
- Journal events locally; roll snapshots periodically; reconcile at project boundaries.
- Use 3‑way merges for notes/config; last‑writer‑wins only for low‑risk state.

See also: Offline & Sync, Replay & Time Travel, Artifacts & Provenance.
