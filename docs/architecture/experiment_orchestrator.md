---
title: Experiment Orchestrator
---

# Experiment Orchestrator
Updated: 2025-09-14
Type: Explanation

First‑class A/B/n and shadow runs with assignment rules, per‑run budgets, and deltas reported to `/state/experiments`.

Concepts
- Experiment: id, name, variants[], status, metrics.
- Assignment: per agent/project/run; canary per project; TTL.
- Budgets: token/time/cost caps per variant.

Lifecycle Events
- `experiment.started`, `experiment.variant.chosen`, `experiment.result`, `experiment.completed`.

Read‑Models
- `/state/experiments` shows recent lifecycle events and results.

Endpoints (planned)
- `POST /experiments/start` — define variants and assignment rules.
- `POST /experiments/stop` — finalize and persist results.
- `POST /experiments/assign` — override assignment for a session.

UI
- A/B dry‑run in Logic Units Library; deltas displayed inline (solve‑rate, latency, token spend, diversity).

See also: Logic Units, Evaluation Harness, Budgets & Context Economy.
