---
title: Security Posture
---

# Security Posture (Plan)

Updated: 2025-09-12

Per‑project security posture presets combine policy, egress, and lightweight mitigations.

Modes
- Relaxed: dev‑friendly; prompts for risky permissions; minimal quarantines; local‑only preferred.
- Standard: default; memory quarantine on; project isolation; egress posture “Public only”; headless browsing hardened; DNS guard enabled.
- Strict: disable remote JS; block non‑HTTP protocols for tools; require manual review for world diffs; stronger secrets redaction; deny IP‑literals; enforced archive jails; accelerator zeroing.

Planned configuration
- `ARW_SECURITY_POSTURE`: `relaxed|standard|strict` (per project)

See also: Network Posture; Lightweight Mitigations; Security Hardening.

