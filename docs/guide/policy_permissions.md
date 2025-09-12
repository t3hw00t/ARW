---
title: Permissions & Policies
---

# Permissions & Policies

Safe by default, fluid in use. Capabilities are explicit; policies grant/deny with optional TTL leases and audit flags. Tools declare what they need; policy gates appear as inline prompts in the sidecar when escalation is required.

Capabilities (examples)
- `fs:read`, `fs:write`, `net:http`, `shell:exec`, `mic`, `cam`, `gpu`, `sandbox:<kind>`

Modes
- `ask` (prompt), `allow` (auto), `never` (hard deny), with optional TTLs (e.g., 15 min)

Scopes
- File scopes: `project://read`, `vault://secrets/*`, `path://~/Documents/*.pdf`
- Network allow‑list: `net://example.com`, `net://api.example.com:443`

Auditability
- Every allow/deny decision is an event; sidecar renders a reviewable history for each episode

Related docs
- Policy internals and capsules: `docs/POLICY.md`
- Security Hardening: `docs/guide/security_hardening.md`

Tauri v2 mapping
- Model Tauri’s capabilities/permissions to match ARW policies. Example: expose only `fs:read` and a file picker when ARW grants `fs:read` with an active TTL lease; block APIs otherwise.
- Keep UI bridges (e.g., notifications, tray, deep links) behind policy prompts; surface the prompt and decision inline in the sidecar so users can review why access was granted.
