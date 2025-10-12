---
title: Offline & Sync
---

# Offline & Sync

Updated: 2025-10-10
Type: How‑to

Local‑first
- Local read‑models + event journal are authoritative; UI rehydrates from them.

Sync boundaries
- Project‑scoped synchronization; explicit export/import; conflict resolution rules.

Conflicts
- Last‑writer‑wins only for non‑critical state; 3‑way merges for notes/config.

- Docs bootstrap
  - Releases include `docs-wheels.tar.gz`; extract and run `scripts/bootstrap_docs.sh --wheel-dir <dir>` to hydrate MkDocs offline (or use `mise run bootstrap:docs -- --wheel-dir <dir>`).

See also: Artifacts & Provenance, Naming & IDs.
