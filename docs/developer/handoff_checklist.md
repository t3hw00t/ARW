---
title: Handoff Checklist — Policy & Egress Restructure
---

# Handoff Checklist — Policy & Egress Restructure

Updated: 2025-10-09
Type: Reference

Scope
- Performance presets; Policy posture; Egress proxy + DNS guard; Settings control plane; Correlation; Docs.

Runbook
- Build: `cargo build --workspace` (or `just dev-server` for the unified server)
- Start unified server: `ARW_PORT=8091 cargo run -p arw-server`
- Proxy/DNS guard now default on (`ARW_EGRESS_PROXY_ENABLE=1`, `ARW_DNS_GUARD_ENABLE=1`); set them to `0` only if you intentionally disable enforcement. Ledger remains opt-in via `ARW_EGRESS_LEDGER_ENABLE=1`.
- Inspect settings: `GET /state/egress/settings`
- Update settings: `POST /egress/settings` (admin header required)
- Preview egress: `POST /egress/preview` with `{ url, method? }`
- Stream events: `GET /events?prefix=egress.`
- SSE smoke test: `ARW_ADMIN_TOKEN=... ./scripts/sse_smoke.sh` (respects `SSE_SMOKE_TIMEOUT_SECS` or the shared `SMOKE_TIMEOUT_SECS`; set to `0` for long-lived tails)
- Smoke helpers share the timeout harness in `scripts/lib/smoke_timeout.sh`; tweak the guard there when adjusting defaults across shells.

New/Changed Endpoints
- `POST /actions` (idempotent; emits `actions.submitted`)
- `GET /events` (SSE; `?replay` and `?after`; `ARW_EVENTS_SSE_MODE`) 
- `GET /state/egress`
- `GET /state/egress/settings`
- `POST /egress/settings`
- `POST /egress/preview`

Environment (Key)
- Perf: `ARW_PERF_PRESET`, `ARW_HTTP_MAX_CONC`, `ARW_ACTIONS_QUEUE_MAX`, `ARW_CONTEXT_K`
- Policy: `ARW_POLICY_FILE`, `ARW_SECURITY_POSTURE`
- Egress: `ARW_EGRESS_PROXY_ENABLE`, `ARW_EGRESS_PROXY_PORT`, `ARW_DNS_GUARD_ENABLE`, `ARW_EGRESS_BLOCK_IP_LITERALS`, `ARW_EGRESS_LEDGER_ENABLE`, `ARW_NET_ALLOWLIST`
- SSE: `ARW_EVENTS_SSE_MODE=envelope|ce-structured`

Data & Schemas
- Kernel: egress ledger includes `corr_id`, `proj`, `posture`
- Schemas:
  - [spec/schemas/egress_settings.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/egress_settings.json)
  - [spec/schemas/policy_network_scopes.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/policy_network_scopes.json)
  - [spec/schemas/egress_ledger.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/egress_ledger.json)
  - [configs/schema_map.json](https://github.com/t3hw00t/ARW/blob/main/configs/schema_map.json) maps `egress` → `egress_settings.json`

Correlation
- Use headers `X-ARW-Corr` and `X-ARW-Project` for proxy requests to tag ledger rows and events.
- Local worker tags ledger rows with action ids.

Docs
- How‑to: Egress Settings; Subscribe to Events (SSE); Correlation & Attribution
- Architecture: Egress Firewall; Events Vocabulary
- Guide: Admin Endpoints (adds egress endpoints section)
- Dashboards & alerts: re-import `docs/snippets/grafana_quick_panels.md` and `docs/snippets/prometheus_alerting_rules.md` so the legacy `/debug` panels/alerts stay retired. Confirm staging Grafana shows only the capsule-header stat.

Legacy Follow-ups
- Staging smoke: hit `/debug` (expect 404) and watch `arw_legacy_capsule_headers_total` for 24h before cutting prod traffic.
- Ops runbook: note `/admin/debug` as the only UI entry; remove leftover `/debug` bookmarks in internal docs or scripts.

Next
- Cedar ABAC integration; richer ledger CE; UI hooks for settings; DNS guard policy bindings.
