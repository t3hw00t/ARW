---
title: Agent Orchestrator
---

# Agent Orchestrator
Updated: 2025-10-04
Type: Explanation

Purpose
- Train and manage “mini‑agents” that coordinate other autonomous agents under policy and budgets. The orchestrator plans training episodes, runs them through the Evaluation Harness, and promotes configurations to Logic Units.

Principles
- Safe by design: all training/offload is lease‑gated; no unbounded egress or code installs.
- Reproducible: episodes recorded with corr_id; artifacts and decisions attributed; snapshots exportable.
- Measurable: solve‑rate, latency, token spend, memory utility; confidence calibration.

Architecture
- Orchestrator service schedules training jobs, pulls goals/datasets, runs episodes via the unified triad (/actions,/events,/state), and writes back Logic Units patches.
- Produces: mini‑agent profiles, candidate Logic Units, evaluation reports, and promotion decisions.
- Consumes: Memory Abstraction Layer (for curriculum and retrieval), Policies/Leases, and the Evaluation Harness.

Endpoints (initial stubs)
- `GET /orchestrator/mini_agents` → `{ items: [...] }`
- `POST /orchestrator/mini_agents/start_training` → `{ job_id }` (admin‑gated; 501 until runner lands)
- `GET /state/orchestrator/jobs` → `{ items: [...] }` (includes submitted training hints per job, which are also applied to `governor.hints` when the run starts)

Roadmap
- Phase 1: integrate with Evaluation Harness; record results; output Logic Unit patches.
- Phase 2: curriculum & self‑play; memory‑guided sampling and active learning.
- Phase 3: promotion policy and confidence‑aware routing across agent teams.

See also: Logic Units, Evaluation Harness, Memory Abstraction Layer, Policy & Leases.
