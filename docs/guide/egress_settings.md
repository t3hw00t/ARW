---
title: Egress Settings
---

# Egress Settings
Updated: 2025-10-09
Type: How‑to

Control the egress proxy, DNS guard, IP‑literal blocking, allowlist, and structured network scopes (domains/IPs/ports/protocols with optional TTL) at runtime, and persist these settings via the unified config snapshot.

Endpoints (service)
- `GET /state/egress/settings` → runtime summary `{ egress: {...}, recommended: {...}, capsules: {...}, leases: {...} }`
- `POST /egress/settings` (admin‑gated) → update toggles and persist to config under `egress` (validated against [spec/schemas/egress_settings.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/egress_settings.json))
- `POST /egress/preview` → dry‑run a URL+method against policy/guards: `{ allow, reason?, host, port, protocol }`

Dynamic proxy
- The proxy starts/stops/rebinds to `proxy_port` immediately after a successful settings update. No restart needed.

Examples
```bash
# Inspect
curl -s http://127.0.0.1:8091/state/egress/settings | jq

# Sample
# {
#   "egress": {
#     "posture": "allowlist",
#     "allowlist": ["api.github.com"],
#     "block_ip_literals": true,
#     "dns_guard_enable": true,
#     "proxy_enable": true,
#     "proxy_port": 9080,
#     "ledger_enable": true,
#     "multi_label_suffixes": ["internal.test","gov.bc.ca"],
#     "scopes": [
#       {
#         "id": "github",
#         "hosts": ["api.github.com","*.githubusercontent.com"],
#         "ports": [443],
#         "protocols": ["https"],
#         "lease_capabilities": ["net:https","net:http"],
#         "expires_at": "2025-12-01T00:00:00Z"
#       }
#     ]
#   },
#   "recommended": {
#     "block_ip_literals": true,
#     "dns_guard_enable": true,
#     "proxy_enable": true,
#     "ledger_enable": true,
#     "multi_label_suffixes": []
#   },
#   "capsules": {
#     "active": 1,
#     "snapshot": { ... }
#   },
#   "leases": {
#     "total": 3,
#     "net": 2,
#     "items": [ ... ]
#   }
# }

# Enable proxy + ledger + DNS guard, allow only GitHub API, add internal suffixes, persist
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
        "ledger_enable": true,
        "multi_label_suffixes": ["internal.test","gov.bc.ca"],
        "scopes": [
          {
            "id": "github",
            "hosts": ["api.github.com","*.githubusercontent.com"],
            "ports": [443],
            "protocols": ["https"],
            "lease_capabilities": ["net:https","net:http"],
            "expires_at": "2025-12-01T00:00:00Z"
          }
        ]
      }' | jq

# Preview a request before running tools
curl -s -X POST http://127.0.0.1:8091/egress/preview \
  -H 'content-type: application/json' \
  -d '{"url":"https://api.github.com","method":"GET"}' | jq

# Inspect scopes via CLI
arw-cli admin egress scopes --base http://127.0.0.1:8091

# Emit raw JSON instead of text
arw-cli admin egress scopes --json --pretty

# Add a scope that allows GitHub over HTTPS and mints a lease
arw-cli admin egress scope add \
  --id github \
  --host api.github.com --host '*.githubusercontent.com' \
  --protocol https --port 443 \
  --lease-cap net:https \
  --expires-at 2025-12-01T00:00:00Z

# Update an existing scope description and lease capabilities
arw-cli admin egress scope update --id github \
  --description "GitHub API" \
  --lease-cap net:https --lease-cap net:domain:github.com

# Remove a scope
arw-cli admin egress scope remove --id github
```

Notes
- Settings persist in the server config snapshot under `egress`; use the Config Plane or `/patch/*` to manage snapshots.
- `multi_label_suffixes` entries should be effective TLDs (e.g., `internal.test` or `gov.bc.ca`). The runtime automatically prepends the registrant label when collapsing hostnames.
- When `proxy_enable=1`, built‑in `http.fetch` routes via `127.0.0.1:proxy_port` automatically.
- Add correlation headers (`X-ARW-Corr`, `X-ARW-Project`) to proxy requests to tag ledger rows and events.
- Changing `posture` without specifying booleans adopts the `recommended` defaults (block IP literals, DNS guard, proxy, and ledger) so posture/ledger stay aligned. Provide explicit values in the patch body to override.
- `scopes` supplement posture allowlists with structured network grants. Entries first expire client-side (`expires_at`) and are ignored without deleting them, so you can pre-stage time-bound access.
- Optional `lease_capabilities` on a scope mints/refreshes the listed capability leases whenever that scope allows traffic, so you can mirror policy intent into the lease ledger automatically.

Network scopes
- Define `hosts` (exact or wildcard) and/or `cidrs` to describe the endpoints covered by the scope. Empty entries are ignored.
- Optional `ports` limit the scope to specific TCP ports. When omitted the posture defaults apply.
- Optional `protocols` support `http`, `https`, or `tcp` (tcp matches both HTTP and HTTPS).
- `expires_at` accepts an ISO 8601 timestamp. Once the timestamp is past, the server ignores the scope until you refresh it.
- Scopes are validated with the same schema as the rest of the settings; invalid entries surface as `400 Bad Request` responses with per-field diagnostics.
- `/egress/preview` responses and proxy ledger rows include `policy_scope` metadata when a scope grant allows the request (mirroring scope id/description/expiry).
- `/state/egress` surfaces `allowed_via` and `policy_scope` alongside each ledger row so dashboards can highlight whether a lease, scope, or base policy allowed the offload.

Schema
- See [spec/schemas/egress_settings.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/egress_settings.json) for the JSON Schema used to validate settings.
