---
title: Logic Units Library
---

# Logic Units Library
Updated: 2025-09-14
Type: How‑to

A first‑class surface to install, try, apply, and govern Logic Units (config‑first strategy packs) safely.

Tabs
- Installed: units available for binding to agent slots.
- Experimental: approved trials with guardrails.
- Suggested: drafts from the Research Watcher or curators.
- Archived: retired units with provenance.

Flows
- Preview: explainer + risk badge + exact config diff.
- Dry‑run: A/B (or A/B/n) on a small benchmark or saved task; show deltas in accuracy/latency/cost.
- Stage: apply to one agent/project with rollback conditions; emit `logic.unit.applied`.
- Promote: make default for a slot; emit `logic.unit.promoted`; provenance records updated.

Composer
- Bind units to Agent Profile slots (Retrieval, Reasoning, Sampling, Policy, Memory, Evaluation); compatibility checker warns on conflicts.

Metrics panel
- Show expected vs observed effects: solve‑rate, tool success, retrieval diversity, latency, cost; link to Evaluation Harness reports.

Permissions
- Units cannot silently widen permissions. If a unit requests capabilities, the Permission Manager prompts inline with TTL leases.

See also: Logic Units (architecture), Evaluation Harness, Permissions & Policies.
