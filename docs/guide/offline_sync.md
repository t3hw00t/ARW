---
title: Offline & Sync
---

# Offline, Caching, and Sync Semantics

Local‑first
- Local read‑models + event journal are authoritative; UI rehydrates from them.

Sync boundaries
- Project‑scoped synchronization; explicit export/import; conflict resolution rules.

Conflicts
- Last‑writer‑wins only for non‑critical state; 3‑way merges for notes/config.

See also: Artifacts & Provenance, Naming & IDs.

