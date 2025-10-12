---
title: Experiment Orchestrator
---

# Experiment Orchestrator
Updated: 2025-09-20
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

Endpoints
- `POST /admin/experiments/define` — register an experiment with variants and knobs.
- `POST /admin/experiments/start` — begin a live run with optional assignment/budget hints.
- `POST /admin/experiments/stop` — finalize and persist results.
- `POST /admin/experiments/assign` — override assignment for a session.
- `POST /admin/experiments/run` — run A/B/n on project goldens (returns `RunOutcome { …, job_id }` and records an `experiment_run` entry in the orchestrator job plane).
- `POST /admin/experiments/activate` — apply winner hints to the governor.
- `GET /admin/experiments/list` — list definitions and variants.
- `GET /admin/experiments/scoreboard` — last-run metrics per variant.
- `GET /admin/experiments/winners` — persisted winners snapshot.

UI
- A/B dry‑run in Logic Units Library; deltas displayed inline (solve‑rate, latency, token spend, diversity).

See also: Logic Units, Evaluation Harness, Budgets & Context Economy.
