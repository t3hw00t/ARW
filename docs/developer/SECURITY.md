---
title: Developer Security Notes
---

# Developer Security Notes
{ .topic-trio style="--exp:.4; --complex:.6; --complicated:.8" data-exp=".4" data-complex=".6" data-complicated=".8" }

Updated: 2025-09-16
Type: Reference

## Surface
- Bind: loopback only (127.0.0.1) by default.
- Sensitive endpoints: gated by `ARW_DEBUG=1` or `X-ARW-Admin` header matching `ARW_ADMIN_TOKEN`.
- CORS: permissive only if `ARW_DEBUG=1` or `ARW_CORS_ANY=1`; otherwise restrictive.

## Sensitive Endpoints
- `/admin/debug`, `/admin/probe*`, `/admin/memory*`, `/admin/models*`, `/admin/governor*`, `/admin/introspect*`, `/admin/chat*`, `/admin/feedback*`, `/admin/events`, `/admin/emit*`, `/admin/shutdown`.
- Adjust the list conservatively; prefer to over‑protect and open case‑by‑case.

## Recommendations
- Development: set `ARW_DEBUG=1` locally; do not expose ports publicly.
- Hardened usage: unset `ARW_DEBUG`, set `ARW_ADMIN_TOKEN`, require `X-ARW-Admin`.
- Consider reverse proxy with TLS and IP allowlist if remote.
- Keep hints/profile and suggestions in the state dir; avoid secrets in suggestions.

## Next
- Rate‑limits for admin endpoints; structured audit events; optional signed capsules.
- Policy engine (OPA/Cedar) for consistent, verifiable authorization.
- Egress gateway (plan): host‑local proxy + DNS guard with policy network scopes and TTL leases; default posture "Public only" for projects.
