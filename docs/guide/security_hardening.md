---
title: Security Hardening
---

# Security Hardening

ARW ships with conservative defaults and clear toggles to harden deployments without breaking development.

## Admin Surfaces

- Admin endpoints (`/admin/*`) and admin UI assets (`/admin/ui/*` and `/admin/ui/assets/*`) require an admin token by default.
- Debug mode (`ARW_DEBUG=1`) is loopback‑only: admin access is granted from `127.0.0.1/::1` without a token; remote callers are denied unless a valid token is supplied.
- Tokens can be set as `ARW_ADMIN_TOKEN` or `ARW_ADMIN_TOKEN_SHA256` (hex). The latter avoids keeping plain tokens in env.
- Rate limits are enforced for admin auth attempts; tune with `ARW_ADMIN_RATE_LIMIT` and `ARW_ADMIN_RATE_WINDOW_SECS`.

## Content Security Policy (CSP)

- Default is relaxed to keep dev panels working: inline scripts and styles allowed.
- Set `ARW_CSP_PRESET=strict` to enable strict CSP on non‑debug pages: blocks inline scripts; allows external scripts from `self` and inline styles for layout.
- Debug pages remain relaxed by default; set `ARW_DEBUG_CSP_STRICT=1` to partially harden them while migrating away from inline handlers.

## Egress Guard

- WASI host blocks DNS‑over‑HTTPS/DoT and optionally IP‑literals.
- In Kubernetes, the Helm chart enables DNS guard and IP‑literal block; add a `NetworkPolicy` egress to restrict outbound network further.

## TLS and Reverse Proxy

- Prefer a TLS‑terminating reverse proxy (Caddy/NGINX) with HSTS in production.
- Ensure `ARW_TRUST_FORWARD_HEADERS=1` when trusting proxy headers like `X‑Forwarded‑For`.

## Helm

- Wire admin token via Secret (recommended). The chart auto‑creates one if `adminToken.value` is set and rolls the Deployment on change.
- Enable `networkPolicy` and `egressPolicy` for least‑privilege network access.

