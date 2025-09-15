---
title: Security Posture
---

# Security Posture

Updated: 2025-09-15
Type: How‑to

Security posture presets are available now and feed the policy engine when an explicit `ARW_POLICY_FILE` is not provided.

Per‑project security posture presets combine policy gates (leases today) and, over time, egress and mitigations.

Modes
- Relaxed: dev‑friendly; prompts for risky permissions; minimal quarantines; local‑only preferred.
- Standard: default; memory quarantine on; project isolation; egress posture “Public only”; headless browsing hardened; DNS guard enabled.
- Strict: disable remote JS; block non‑HTTP protocols for tools; require manual review for world diffs; stronger secrets redaction; deny IP‑literals; enforced archive jails; accelerator zeroing.

Configuration
- `ARW_SECURITY_POSTURE`: `relaxed|standard|strict` (per project). Default: `standard`.

Preset files
- In‑repo JSON presets mirror these postures and can be used directly via `ARW_POLICY_FILE`:
  - `configs/policy/relaxed.json`
  - `configs/policy/standard.json`
  - `configs/policy/strict.json`
  Example: `export ARW_POLICY_FILE=configs/policy/standard.json`

Implementation
- relaxed: `allow_all=true` (dev‑friendly; minimal prompts).
- standard: lease‑gate sensitive prefixes: `net.http.*`, `net.tcp.*`, `fs.*`, `context.rehydrate*`, `app.*`, `tools.browser.*`, `models.download`, `shell.*`.
- strict: lease‑gate most effectors with coarse prefixes: `net.*`, `fs.*`, `context.*`, `models.*`, `tools.*`, `app.*`, `shell.*`, `system.*`.

Override with a policy file by setting `ARW_POLICY_FILE` to a JSON document; this takes precedence over posture.

See: Policy (ABAC Facade).

See also: Network Posture; Lightweight Mitigations; Security Hardening.
