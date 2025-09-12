---
title: Scheduler & Governor
---

# Scheduler & Governor

Purpose
- Fair queues per device/project, preemption, backpressure, and kill‑switches. Admit by budget; degrade gracefully when over plan.

Read‑models
- `/state/runtime_matrix` supplies health/throughput; governor exposes profiles and hints.

Policies
- Respect GPU/sandbox caps and capability leases when scheduling.

See also: Runtime Matrix, Scheduling & Backpressure, Budgets & Context Economy.

