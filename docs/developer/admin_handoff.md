---
title: Admin Handoff — Quick Links & Commands
---

# Admin Handoff — Quick Links & Commands

Updated: 2025-09-15
Type: Reference

Start here to operate the restructured unified server slice. This page links the most relevant docs and includes copy‑paste commands for common tasks.

Run
- Unified server (dev): `ARW_PORT=8091 cargo run -p arw-server`
- Perf presets: `ARW_PERF_PRESET=balanced|performance|turbo|eco`
- Admin token (recommended): `export ARW_ADMIN_TOKEN=...` and send `X-ARW-Admin` or `Authorization: Bearer ...`

Egress
- Inspect settings: `GET /state/egress/settings`
- Update settings: `POST /egress/settings` (admin)
- Preview a URL: `POST /egress/preview`
- Stream ledger events: `GET /events?prefix=egress.&replay=5`
- Script: `ARW_ADMIN_TOKEN=... ./scripts/sse_smoke.sh`

Correlation
- Add headers on proxy’d requests: `X-ARW-Corr`, `X-ARW-Project`
- Events carry `corr_id`, `proj`, `posture`; `/state/egress` rows include the same.

Config & Patch Engine
- Persisted settings live under top‑level `egress` (validated by `spec/schemas/egress_settings.json`).
- Schema map (`configs/schema_map.json`) maps `egress` → `spec/schemas/egress_settings.json`.

Docs Index
- How‑to: Egress Settings
- How‑to: Subscribe to Events (SSE)
- How‑to: Correlation & Attribution
- Architecture: Egress Firewall
- Architecture: Config Plane & Patch Engine
- Reference: API Reference (Egress section)
- Reference: Topics and Events Vocabulary

Justfile helpers
- `just egress-get`
- `just egress-set '{"proxy_enable":true,"proxy_port":9080}'`
- `just egress-proxy-on port=9080`
- `just egress-proxy-off`

Environment quick ref
- Egress: `ARW_EGRESS_PROXY_ENABLE`, `ARW_EGRESS_PROXY_PORT`, `ARW_DNS_GUARD_ENABLE`, `ARW_EGRESS_BLOCK_IP_LITERALS`, `ARW_EGRESS_LEDGER_ENABLE`, `ARW_NET_ALLOWLIST`
- Policy posture: `ARW_SECURITY_POSTURE`
- SSE format: `ARW_EVENTS_SSE_MODE=envelope|ce-structured`
- Performance: `ARW_PERF_PRESET`

