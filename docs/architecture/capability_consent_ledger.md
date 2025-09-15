---
title: Capability & Consent Ledger
---

# Capability & Consent Ledger
Updated: 2025-09-14
Type: Explanation

Purpose
- Auditable, time‑bound permissions the UI can always explain.

Model
- Grants are leases with `{ capability, scope, ttl_secs, issued_at, issued_by, reason }`.
- Events: `policy.prompt`, `policy.allow`, `policy.deny`, `policy.expired`.
- Denials and escalations are first‑class events.

Storage
- Journaled locally; summarized for UI. Planned endpoint: `/state/policy` with active leases per principal.

UI
- Inline prompts in the sidecar; history visible per episode and per project.

Tauri mapping
- Expose Tauri v2 capabilities only when a matching ARW lease is active.

See also: Permissions & Policies, Identity & Tenancy, Threat Model.
