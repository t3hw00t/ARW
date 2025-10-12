---
title: Connectors (Cloud & Local Apps)
---

# Connectors (Cloud & Local Apps)
Updated: 2025-10-12
Type: How‑to

Safely connect agents to cloud apps and local desktop apps with explicit scopes and leases.

Concepts
- Connector: a registered integration with a `provider` and `scopes` (e.g., `github` with `repo:rw`).
- Token: a credential tied to a connector id. Managed out of band; stored locally under `state/connectors/*.json`.
- Leases: time‑boxed capability grants that allow using a connector or launching a local app action.

API (unified server)
- List connectors: `GET /state/connectors`
- Register: `POST /connectors/register` (admin‑gated)
  - Body: `{ "id?":"gh-main", "kind":"cloud", "provider":"github", "scopes":["repo:rw"], "meta":{ "note":"personal" } }`
- Set token: `POST /connectors/token` (admin‑gated)
  - Body: `{ "id":"gh-main", "token":"gho_...", "expires_at":"2026-01-01T00:00:00Z" }`

Security & policy
- Secrets redaction: `/state/connectors` never returns `token`/`refresh_token`.
- Egress: calls using connectors still obey allowlists and leases (e.g., `net:http:api.github.com`).
- Scopes → leases: every scope declared on a connector expects a matching capability lease (e.g., grant `cloud:github:repo:rw` before invoking a GitHub connector).
- Local apps: treat local app actions as tools with tight leases (e.g., `io:app:vscode`, `io:app:word`).
- No auto‑install: adding tokens requires `ARW_ADMIN_TOKEN` by default.

Examples
### Register a GitHub connector and set a token
```bash
curl -s -X POST localhost:8091/connectors/register \
  -H 'content-type: application/json' -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  -d '{"id":"gh-main","kind":"cloud","provider":"github","scopes":["repo:rw"],"meta":{}}'

curl -s -X POST localhost:8091/connectors/token \
  -H 'content-type: application/json' -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  -d '{"id":"gh-main","token":"gho_xxx","expires_at":"2026-01-01T00:00:00Z"}'

curl -s localhost:8091/state/connectors | jq

### Use a connector with http.fetch
```bash
curl -s -X POST localhost:8091/leases -H 'content-type: application/json' \
  -d '{"capability":"cloud:github:repo:rw","ttl_secs":600}' | jq

curl -s -X POST localhost:8091/actions -H 'content-type: application/json' \
  -d '{
        "kind":"net.http.get",
        "input":{ "url":"https://api.github.com/user", "connector_id":"gh-main" }
      }' | jq
```
The runtime injects `Authorization: Bearer <token>` and still enforces egress allowlists. If the lease is missing, the action returns `connector lease required` and emits a `policy.decision`. Optionally restrict hosts per connector by setting `meta.allowed_hosts` in the connector manifest (e.g., `["api.github.com"]`).

### Optional SearXNG metasearch connector {#optional-searxng-metasearch-connector}

Install SearXNG locally (example Docker Compose):

```bash
mkdir -p searxng && cd searxng
cat > docker-compose.yml <<'EOF'
services:
  searxng:
    image: searxng/searxng:2025.2.0
    restart: unless-stopped
    ports:
      - "8080:8080"
    environment:
      - BASE_URL=http://127.0.0.1:8080/
      - INSTANCE_NAME=ARW Metasearch
      - SEARXNG_SETTINGS_SERVER_SECRET_KEY=$(openssl rand -hex 32)
EOF
docker compose up -d
```

Register a connector manifest and grant a lease:

```bash
curl -s -X POST localhost:8091/connectors/register \
  -H 'content-type: application/json' -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  -d '{
        "id":"search-searxng",
        "kind":"service",
        "provider":"searxng",
        "scopes":["cloud:searxng:search"],
        "meta":{
          "base_url":"http://127.0.0.1:8080",
          "search_path":"/search",
          "format":"json",
          "allowed_hosts":["127.0.0.1","localhost"]
        }
      }'

curl -s -X POST localhost:8091/leases -H 'content-type: application/json' \
  -d '{"capability":"cloud:searxng:search","ttl_secs":900}' | jq

curl -s -X POST localhost:8091/actions -H 'content-type: application/json' \
  -d '{
        "kind":"net.http.get",
        "input":{
          "url":"http://127.0.0.1:8080/search?q=open+source&format=json",
          "connector_id":"search-searxng"
        }
      }' | jq
```

SearXNG handles all outbound search traffic while ARW only calls the local aggregator. Use `arw-cli http fetch --connector-id search-searxng --preview-kb 128 "http://127.0.0.1:8080/search?q=arw&format=json"` or wire the connector into browsing recipes for richer research flows.

> Sample manifest: [`examples/connectors/searxng.json`](https://github.com/t3hw00t/ARW/blob/main/examples/connectors/searxng.json) — additional connectors live in the [Connector Catalog](../reference/connectors.md).

### Local apps (first tool; more planned)
- `app.vscode.open` — opens a folder/file in VS Code (lease: `io:app:vscode`). Example:
```bash
curl -s -X POST localhost:8091/leases -H 'content-type: application/json' \
  -d '{"capability":"io:app:vscode","ttl_secs":600}' | jq

curl -s -X POST localhost:8091/actions -H 'content-type: application/json' \
  -d '{"kind":"app.vscode.open","input":{"path":"projects/demo"}}' | jq
```
- Planned additions: `app.word.open`, `app.mail.compose`, and other desktop bridges will ship with the capability leases noted above once hardened. Track progress in the Restructure Handbook.

Notes
- OAuth helpers are planned; today tokens are set directly via `POST /connectors/token`.
- Prefer PATs or app tokens with minimal scopes and TTL.

See also: Egress Firewall, Policy (ABAC Facade), Security Hardening.
