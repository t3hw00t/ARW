---
title: Scheduling & Backpressure
---

# Scheduling, Concurrency, and Backpressure

Principles
- Prevent stampedes, deadlocks, and device starvation; enable graceful degradation.

Queues
- Per‑project and per‑device queues with priority and fairness.
- Idempotent retries with SLO‑aware backoff; circuit breakers around flaky tools.

See also: Guide → Performance & Reasoning Playbook (modes, SLOs, admission, degradation).

Kill‑switch
- Global and per‑project kill‑switch events; visible in the sidecar.

Policy‑aware scheduling
- Integrate with the Runtime Matrix to respect GPU/sandbox caps and policy leases.

See also: Runtime Matrix, Cost & Quotas, Policy.
