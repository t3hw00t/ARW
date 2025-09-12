---
title: AppSec Harness
---

# AppSec Harness (Policy‑Integrated)

Purpose
- Bake LLM‑specific security checks into development and runtime: prompt‑injection, insecure output handling, tool SSRF, data leakage.

Approach
- Maintain a checklist and test corpus (seeded cases) aligned with OWASP LLM Top‑10 and the OWASP GenAI project.
- Surface violations as `Policy.Decision` events and block at policy gates.

Components
- Test kit: run seeded prompts/tools; expect denials or sanitization.
- Runtime guards: content‑policy filters before memory or actions.
- Reporting: per‑episode violation summary; per‑project trend.

See also: Permissions & Policies, Threat Model, Human‑in‑the‑Loop.

