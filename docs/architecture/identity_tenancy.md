---
title: Identity & Tenancy
---

# System Identity & Tenancy

Principals
- User: the human operator on this machine.
- Project: a scoped workspace rooted at a folder (files, notes, data, memories).
- AgentInstance: a running binding of an Agent Profile + Runtime + Policy within a Project.

Scoping rules
- Capabilities, storage paths, caches, and logs are scoped to one or more principals.
- Default scope for actions: AgentInstance → Project → User (narrowest first).

Paths
- `user://` (per‑user state); `project://` (within project root); `agent://` (ephemeral/runtime scoped).

Policy & UI
- Prompts cite the scope (“Allow net:http for Agent X in Project Y for 15 min?”).
- Decisions are events with `{ principal_scope, ttl, policy_id }`.

See also: Permissions & Policies, Data Governance, Naming & IDs.

