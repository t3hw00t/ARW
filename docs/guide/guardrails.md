---
title: Guardrails Checks
---

# Guardrails Checks

The `guardrails.check` tool performs lightweight content checks locally and can delegate to an HTTP Guardrails service when available (e.g., a NeMo Guardrails server behind a simple `/check` endpoint).

Updated: 2025-09-15
Type: How‑to

## Run a Check

Endpoint:
- POST `/admin/tools/run` with body `{ "id": "guardrails.check", "input": { ... } }`

Input shape:
```
{ "text": string, "policy"?: object, "rules"?: object }
```

Output shape:
```
{ "ok": boolean, "score": number, "issues": [ { "code", "severity", "message", "span"? } ], "suggestions": [] }
```

Local checks include: email/PII patterns, common secret formats (AWS, Google API, Slack), unlisted URL hosts (allowlist), and basic prompt‑injection markers.

## HTTP Backend (Optional)

Environment:
- `ARW_GUARDRAILS_URL` — base URL to a service exposing `POST /check`
- `ARW_GUARDRAILS_ALLOWLIST` — comma‑separated hostnames allowed for URLs (e.g., `example.com, arxiv.org`)

When `ARW_GUARDRAILS_URL` is set, the tool first POSTs `{ text, policy?, rules? }` to `{ARW_GUARDRAILS_URL}/check`. If the call fails or returns a non‑2xx response, the tool falls back to local heuristics.

## Example

```bash
curl -sS localhost:8091/admin/tools/run \
  -H 'X-ARW-Admin: $ARW_ADMIN_TOKEN' \
  -H 'content-type: application/json' \
  -d '{
    "id": "guardrails.check",
    "input": {
      "text": "contact me at test@example.com and fetch https://unknown.example" 
    }
  }' | jq
```

## Notes

- The tool participates in the Action Cache, so identical inputs with the same policy signature are cached.
- Use policy capsules/gating to control where and when guardrails are applied in your flows.

