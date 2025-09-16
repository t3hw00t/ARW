---
title: Self‑Model (Metacognition)
---

# Self‑Model (Metacognition)
Updated: 2025-09-16
Type: Explanation

Yes—it’s both possible and useful. ARW adopts a scoped, evaluable self‑model that an agent can read and (with guardrails) propose updates to. Think of it as the agent’s capability map + confidence + cost model + limits, kept honest by continuous measurement and policy.

What it is (minimal, practical definition)
- Identity: model family/hash, active logic units, policies/leases in force.
- Capability map: what tools and modalities it can use (declared).
- Competence map: where it tends to succeed (measured per task/domain).
- Calibration: how well confidence matches reality (over/under‑confidence).
- Resource curve: expected tokens/latency/$ for common tasks.
- Failure modes: known weaknesses, red flags that trigger abstain/escalate.
- Interaction contract: style/constraints you require (cite sources, never write to fs without lease, etc.).

How the agent uses it
- Plan selection: choose strategy (logic unit set), local vs remote, and toolchain based on predicted success/cost.
- Risk control: abstain/escalate when predicted error or cost exceeds policy.
- Budget keeping: pick prompts/recipes to stay within token/time constraints.
- Transparency: surface “why I chose this plan” from its self‑model fields.

How you evaluate it (objective, automatable)
- Calibration: Brier score / Expected Calibration Error on the agent’s own confidence estimates.
- Selective prediction: risk–coverage curves (quality when the agent agrees to answer vs abstain).
- Competence tracking: success rate by task type/domain/tool; time‑decayed to catch drift.
- Resource accuracy: MAE between predicted vs actual tokens/latency/cost.
- Safety outcomes: rate of blocked actions caught by policy, hallucination/claim‑verification failures.
- Stability: delta in the self‑model over time; large jumps require review.

Update loop (no weight training required)
- Measure every episode → aggregate to rolling metrics per domain/tool.
- Fit simple calibrators (e.g., temperature/isotonic) on held‑out golden tasks.
- Publish a new self‑model proposal with diffs; require human approval for changes that widen scope or touch policies.
- Time‑decay and reset rules to avoid stale or self‑fulfilling beliefs.

Where it lives in ARW
- As a first‑class read‑model: `/state/self/{agent}` alongside world/project state.
- Populated from the same event stream (episodes, actions, policy decisions, costs).
- Visible in the UI: a compact “Agent Card” (confidence, competence, costs, current leases) and a reliability mini‑chart.
- Patched via config (not code): the agent can propose edits; apply/reject through the same diff+lease workflow as Logic Units.

Guardrails (so it helps rather than harms)
- No self‑granting permissions: the agent can’t widen its own capabilities; it can only request leases with rationale.
- Anti‑gaming: evaluate on hidden goldens and occasional cross‑checks by an assessor agent; keep evaluation and training recipes separate.
- Provenance: every decision cites self‑model fields used; snapshots include the self‑model version for replay.
- Drift alarms: thresholded alerts when calibration degrades or costs diverge from predictions.

Good defaults to ship
- “Metacognition” logic unit that enables confidence estimates + calibration.
- “Abstain/Escalate” gate that uses risk–coverage targets.
- “Resource Forecaster” that predicts tokens/latency for chosen recipes.
- “Failure Modes” list seeded by you (e.g., OCR tables, JS‑heavy sites) that routes to safer plans.

Why this is worth it
- Higher reliability: fewer confident‑wrong answers; safer tool use.
- Better UX: plans and trade‑offs explained in plain terms.
- Cheaper runs: the agent learns which plans waste tokens/time.
- Reproducibility: you can replay an outcome with the self‑model that guided it.

Endpoints & Events (MVP)
- GET `/state/self` → list all stored self‑models; GET `/state/self/{agent}` → JSON model.
- POST `/admin/self_model/propose` → persist a proposal (`self.model.proposed`).
- POST `/admin/self_model/apply` → apply a proposal (`self.model.updated`).

Storage
- Files under `<state>/self/{agent}.json`; proposals under `<state>/self/_proposals/{id}.json`.

See also: Events Vocabulary, Logic Units, Evaluation Harness, Policy & Permissions.
