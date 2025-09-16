---
title: Egress Settings
---

# Egress Settings
Updated: 2025-09-15
Type: How‑to

Control the egress proxy, DNS guard, IP‑literal blocking, and allowlist at runtime, and persist these settings via the unified config snapshot.

Endpoints (service)
- `GET /state/egress/settings` → effective settings `{ posture?, allowlist, block_ip_literals, dns_guard_enable, proxy_enable, proxy_port, ledger_enable }`
- `POST /egress/settings` (admin‑gated) → update toggles and persist to config under `egress` (validated against [spec/schemas/egress_settings.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/egress_settings.json))
- `POST /egress/preview` → dry‑run a URL+method against policy/guards: `{ allow, reason?, host, port, protocol }`

Dynamic proxy
- The proxy starts/stops/rebinds to `proxy_port` immediately after a successful settings update. No restart needed.

Examples
```bash
# Inspect
curl -s http://127.0.0.1:8091/state/egress/settings | jq

# Enable proxy + ledger + DNS guard, allow only GitHub API, persist
curl -s -X POST http://127.0.0.1:8091/egress/settings \
  -H 'content-type: application/json' \
  -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -d '{
        "posture":"standard",
        "allowlist":["api.github.com"],
        "block_ip_literals": true,
        "dns_guard_enable": true,
        "proxy_enable": true,
        "proxy_port": 9080,
        "ledger_enable": true
      }' | jq

# Preview a request before running tools
curl -s -X POST http://127.0.0.1:8091/egress/preview \
  -H 'content-type: application/json' \
  -d '{"url":"https://api.github.com","method":"GET"}' | jq
```

Notes
- Settings persist in the server config snapshot under `egress`; use the Config Plane or `/patch/*` to manage snapshots.
- When `proxy_enable=1`, built‑in `http.fetch` routes via `127.0.0.1:proxy_port` automatically.
- Add correlation headers (`X-ARW-Corr`, `X-ARW-Project`) to proxy requests to tag ledger rows and events.

Schema
- See [spec/schemas/egress_settings.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/egress_settings.json) for the JSON Schema used to validate settings.
