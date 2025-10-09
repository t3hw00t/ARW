---
title: Identity & Tenancy
---

# Identity & Tenancy
Updated: 2025-10-09
Type: Explanation

Principals
- User: the human operator on this machine.
- Project: a scoped workspace rooted at a folder (files, notes, data, memories).
- AgentInstance: a running binding of an Agent Profile + Runtime + Policy within a Project.

Scoping rules
- Capabilities, storage paths, caches, and logs are scoped to one or more principals.
- Default scope for actions: AgentInstance → Project → User (narrowest first).

Paths
- `user://` (per‑user state); `project://` (within project root); `agent://` (ephemeral/runtime scoped).

Configuration
- Declarative principals live in `configs/security/tenants.toml` (override via `ARW_TENANTS_FILE`). Each entry carries hashed (`token_sha256`) secrets, roles, and scope hints.
- Runtime reload polls the tenants file; updates broadcast on `identity.registry.reloaded`.
- Operators can introspect the active registry via `GET /state/identity` (admin token required).
- Use `arw-cli admin identity add/remove/show` to manage the tenants manifest without editing TOML by hand.

Policy & UI
- Prompts cite the scope (“Allow net:http for Agent X in Project Y for 15 min?”).
- Decisions are events with `{ principal_scope, ttl, policy_id }`.

See also: Permissions & Policies, Data Governance, Naming & IDs.
