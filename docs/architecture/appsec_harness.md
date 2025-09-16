---
title: AppSec Harness
---

# AppSec Harness

Updated: 2025-09-15
Type: Explanation

Purpose
- Bake LLM‑specific security checks into development and runtime: prompt‑injection, insecure output handling, tool SSRF, data leakage.

Approach
- Maintain a checklist and test corpus (seeded cases) aligned with OWASP LLM Top‑10 and the OWASP GenAI project.
- Surface violations as `policy.decision` events and block at policy gates.

Components
- Test kit: run seeded prompts/tools; expect denials or sanitization.
- Runtime guards: content‑policy filters before memory or actions.
- Reporting: per‑episode violation summary; per‑project trend.

See also: Permissions & Policies, Threat Model, Human‑in‑the‑Loop.

Network & IO hardening (planned)
- Pair the harness with the Egress Firewall for enforceable least‑privilege at the network boundary.
- Filesystem scoping and sensor leases (mic/cam) become policy‑visible capabilities with TTL.
