---
title: Permissions & Policies
---

# Permissions & Policies
Updated: 2025-09-17
Type: How‑to

Safe by default, fluid in use. Capabilities are explicit; policies grant/deny with optional TTL leases and audit flags. Tools declare what they need; policy gates appear as inline prompts in the sidecar when escalation is required.

Capabilities (examples)
- `fs:read`, `fs:write`, `net:http`, `shell:exec`, `mic`, `cam`, `gpu`, `sandbox:<kind>`
- `io:screenshot` (screen/window/region capture + annotate), `io:ocr`

Modes
- `ask` (prompt), `allow` (auto), `never` (hard deny), with optional TTLs (e.g., 15 min)

Scopes
- File scopes: `project://read`, `vault://secrets/*`, `path://~/Documents/*.pdf`
- Network allow‑list: `net://example.com`, `net://api.example.com:443`

Auditability
- Every allow/deny decision is an event; sidecar renders a reviewable history for each episode
- Screenshot captures publish `screenshots.captured`; annotate/OCR runs log alongside the lease so Activity + Gallery show who requested them.

Related docs
- Policy internals and capsules: see [Glossary → Capsule](../GLOSSARY.md) and [Admin Endpoints](admin_endpoints.md)
- Security Hardening: [security_hardening.md](security_hardening.md)

Tauri v2 mapping
- Model Tauri’s capabilities/permissions to match ARW policies. Example: expose only `fs:read` and a file picker when ARW grants `fs:read` with an active TTL lease; block APIs otherwise.
- Keep UI bridges (e.g., notifications, tray, deep links) behind policy prompts; surface the prompt and decision inline in the sidecar so users can review why access was granted.
