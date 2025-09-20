---
title: Policy (ABAC Facade)
---

# Policy (ABAC Facade)

Updated: 2025-09-17
Type: How‑to

This page documents the interim ABAC (attribute‑based access control) facade used by the unified server. It provides a small, JSON‑based policy engine that can be replaced by Cedar later without changing server call sites.

## Concepts
- Subject: for now, the local node acts as subject `"local"`.
- Capability Leases: time‑boxed, scoped permissions created by `POST /leases` and listed via `GET /state/leases`.
- Decision: `allow` or `deny`, with optional `require_capability` and a JSON `explain` payload for transparency.

## Configuration
Set `ARW_POLICY_FILE` to a JSON file:

```json
{
  "allow_all": false,
  "lease_rules": [
    { "kind_prefix": "net.http.", "capability": "net:http" },
    { "kind_prefix": "context.rehydrate.memory", "capability": "context:rehydrate:memory" },
    { "kind_prefix": "context.rehydrate", "capability": "context:rehydrate:file" },
    { "kind_prefix": "fs.", "capability": "fs" },
    { "kind_prefix": "ui.screenshot.", "capability": "io:screenshot" },
    { "kind_prefix": "ui.screenshot.ocr", "capability": "io:ocr" },
    { "kind_prefix": "app.vscode.", "capability": "io:app:vscode" }
  ]
}
```

- `allow_all`: when true, decisions default to allow.
- `lease_rules`: list of `{ kind_prefix, capability }`. If an action `kind` starts with `kind_prefix`, a valid lease for `capability` is required.
  - Example: gate screenshot tools by mapping `ui.screenshot.` to `io:screenshot` (and `ui.screenshot.ocr` to `io:ocr`).

Presets
- Ready‑to‑use presets are available in this repo:
  - `configs/policy/relaxed.json`
  - `configs/policy/standard.json`
  - `configs/policy/strict.json`
  Export one to use it: `export ARW_POLICY_FILE=configs/policy/strict.json`.

## Server Integration
- `POST /actions`: evaluates policy for the action kind. If a capability is required, verifies a valid lease for subject `local`.
- `POST /context/rehydrate`: when policy says context rehydrate requires a lease, checks for `context:rehydrate:memory` (memory pointers), `context:rehydrate:file`, or a generic `fs` lease.
- `GET /state/policy`: returns the current policy snapshot (from `ARW_POLICY_FILE` or defaults).
- `fs.patch` action: lease‑gated when `fs.*` requires a capability; writes atomically under `state/projects` and emits `projects.file.written`.
 - `app.vscode.open` action: lease‑gated when `app.vscode.*` requires a capability; spawns VS Code to open a path under `state/projects`; emits `apps.vscode.opened`.

### Simulate Decisions
Quickly test how a `kind` would be decided without executing anything:

```bash
curl -s -X POST localhost:8091/policy/simulate \
  -H 'content-type: application/json' \
  -d '{"kind":"net.http.get"}' | jq
```

You can also provide Cedar-like entities (optional):

```bash
curl -s -X POST localhost:8091/policy/simulate \
  -H 'content-type: application/json' \
  -d '{
        "action":"net.http.get",
        "subject": {"kind":"node","id":"local"},
        "resource": {"kind":"action","id":"net.http.get"}
      }' | jq
```

## Leases API
- `POST /leases` — Create a new lease for the local subject (body):
  - `{ "capability": "fs", "scope?": "path=/workspace", "ttl_secs?": 600, "budget?": 100 }`
  - Response: `{ "id": "...", "ttl_until": "..." }`
- `GET /state/leases` — List current leases.

## Migration to Cedar
The facade offers a stable seam to later plug Cedar ABAC:
- Entity model: subjects (nodes/agents), resources (actions, tools, files), attributes (tags, roles), and leases as facts.
- The server uses a single entry point (policy `evaluate_action(kind)` + lease checks) that can be mapped to Cedar authorizer calls.

## Notes
- This facade enforces lease‑based gates for action prefixes and context rehydrate now.
- It is not a full policy language. Cedar integration will add a proper authorizer and entity store.
