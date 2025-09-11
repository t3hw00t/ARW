# Admin Endpoints

ARW exposes a unified admin/ops HTTP namespace under `/admin`. All sensitive routes live here so updates can’t miss gating.

- Base: `/admin`
- Index (HTML): `/admin`
- Index (JSON): `/admin/index.json`
- Public endpoints (no auth): `/healthz`, `/metrics`, `/spec/*`, `/version`, `/about`

## Authentication

- Header: `X-ARW-Admin: <token>`
- Env var: set `ARW_ADMIN_TOKEN` to the expected token value for the service.
- Local dev: set `ARW_DEBUG=1` to allow admin access without a token.
- Optional gating capsule: send JSON in `x-arw-gate` header to adopt a gating context for the request.

Rate limiting:
- Env var `ARW_ADMIN_RL` as `limit/window_secs` (default `60/60`).

## Common Admin Paths

- `/admin/introspect/tools`: list available tools
- `/admin/introspect/schemas/{id}`: schema for a known tool id
- `/admin/introspect/stats`: runtime & route stats (JSON)
- `/admin/events`: SSE event stream
- `/admin/probe`: effective paths & memory snapshot (read‑only)
- `/admin/memory[/*]`: memory get/apply/save/load/limit
- `/admin/models[/*]`: list/save/load/add/delete/default/download
- `/admin/tools[/*]`: list and run tools
- `/admin/feedback[/*]`: feedback engine state & policy
- `/admin/state/*`: observations, beliefs, intents, actions
- `/admin/governor/*`: governor profile & hints
- `/admin/hierarchy/*`: negotiation & role/state helpers
- `/admin/projects/*`: project list/tree/notes
- `/admin/chat[/*]`: chat inspection and send/clear
- `/admin/emit/test`: emit a test event
- `/admin/shutdown`: request shutdown

Note: Depending on build flags and environment, some endpoints may be unavailable or no‑op.

## Examples

Setup:

```bash
export ARW_ADMIN_TOKEN=secret123
BASE=http://127.0.0.1:8090
AH() { curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" "$@"; }
```

- List tools
```bash
AH "$BASE/admin/tools" | jq
```

- Run a tool
```bash
curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"id":"math.add","input":{"a":2,"b":3}}' \
  "$BASE/admin/tools/run" | jq
```

- Apply memory item (accepted)
```bash
curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"kind":"episodic","value":{"note":"hello"}}' \
  -X POST "$BASE/admin/memory/apply" -i
```

- Enqueue a task
```bash
curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
  -H 'Content-Type: application/json' \
  -d '{"kind":"math.add","payload":{"a":1,"b":4}}' \
  -X POST "$BASE/admin/tasks/enqueue" | jq
```

- Stream events (SSE)
```bash
curl -N -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" "$BASE/admin/events?replay=10"
```

## OpenAPI

- `/spec/openapi.yaml` provides an OpenAPI document for many admin endpoints.
- The `/admin` index is the authoritative, live source of admin paths from the running binary.

## Security Guidance

- Keep `ARW_ADMIN_TOKEN` secret and rotate routinely.
- Avoid `ARW_DEBUG=1` outside local dev.
- Place ARW behind a reverse proxy with TLS and IP allowlists where possible.
- Consider additional auth at the proxy (mTLS, OIDC) for defense in depth.
