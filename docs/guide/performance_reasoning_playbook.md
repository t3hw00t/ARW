---
title: Performance & Reasoning Playbook
---

# Performance & Reasoning Playbook

Updated: 2025-10-09
Type: How‑to

Goal
- Deliver outputs that are fast when it matters, deep when needed, well‑grounded, and reproducible—by default.

Run Modes (one click, predictable outcomes)
- Quick: local small model, minimal retrieval, no ensembles, strict token cap.
- Balanced (default): local/GPU model, targeted retrieval + re‑rank, light reasoning.
- Deep: bigger model or remote offload for a single “merge/synthesis” step, heavier retrieval, self‑consistency.
- Verified: two diverse models or a second pass that verifies claims against sources; slower, highest trust.

Where this maps in ARW
- Scheduler & Runtime Matrix pick local vs offload respecting SLOs and policy. See Guide → Runtime Matrix and Architecture → Scheduler & Governor.
- Deep/Verified appear as toggles in the debug UI header (and via `/admin/governor/hints`) with pre-offload previews and egress metering.
- Downloads, tools, and egress use budgets and admission checks today (Models, Egress Ledger). See Guide → Models Download, Reference → Egress Schemas.

Scheduling and model/runtime choice (automatic, explainable)
- Define SLOs per project: interactive ≤ 1.5 s P95; batch ≤ 30 s.
- Choose target from the Runtime Matrix:
  - If predicted latency ≤ SLO and context ≤ local limit → keep local.
  - Else if context > local limit or predicted quality gain > threshold → offload one step (Deep mode).
- Always show: chosen target, reason, predicted tokens/latency/cost.

Context assembly that won’t blow the window
- Budget split (defaults; adjustable by Logic Unit):
  - 20% instructions/policy and task constraints
  - 15% current plan/tool I/O stubs
  - 50% evidence (retrieval)
  - 5% world/assumptions
  - 10% reserve (for “rehydrate on demand”)
- Retrieval: Fuse BM25 + dense; apply MMR to enforce diversity. Cap to top‑k=12 slices per turn (Balanced), k=20 (Deep). Always include pointers (IDs) so the agent can rehydrate one more slice instead of inflating the next prompt.
- Compression: Summarize older turns into short, source‑linked bullets. Merge duplicate facts by entity; keep freshest source.
See Architecture → Budgets & Context and Context Working Set.

Reasoning strategies (gated by difficulty)
- Default: ReAct‑style (plan → tool → reflect).
- Turn on self‑consistency automatically when self‑model confidence < 0.6, or task is “multi‑step synthesis”. Defaults: vote‑k=3 (Balanced), k=5 (Deep); stop early if 2 votes agree.
- Use a verifier pass (Verified mode) to check: claims ↔ sources, numbers ↔ units, names ↔ entities in world model.

Confidence, abstention, and escalation
- Calibrate confidence (temperature/isotonic on goldens).
- Gate risky actions by risk–coverage target:
  - If confidence < 0.5 → abstain or ask for approval.
  - 0.5–0.7 → gather one more evidence slice or run light self‑consistency.
  - ≥ 0.7 → proceed under policy.
- Always show confidence and the reason for proceed/abstain/escalate.

Output quality contracts (per output type)
- Research/briefs: claim → citation mapping; contradictions called out; “open questions” listed; summary + appendix of sources.
- Plans/routines: goal, assumptions, steps with tool bindings, time/cost estimate, failure rollbacks.
- Code/automation: action plan, preconditions, idempotency/dedupe key, dry‑run diff, safety check outcomes.
- Data answers: metric definitions, method, sample size/limits, links to artifacts.

Evaluation and drift control (keep it honest)
- Goldens per project (10–50 small tasks). Track: solve rate, latency, token spend; calibration (Brier/ECE); retrieval diversity and coverage.
- AB/Shadow runs only on goldens or archived tasks; promote new Logic Units or recipes only on a measured win.
- Drift alarms when any metric regresses beyond thresholds (e.g., −5% solve rate or +20% latency).
See Guide → Evaluation Harness and Guide → METRICS AND INSIGHTS.

World model assists (grounding without bloat)
- Beliefs are evidence‑backed claims with confidence; retrieval pulls top relevant beliefs instead of full notes.
- If a belief is old or contradicted, schedule a quick refresh rather than stuffing more context. See World Model.

Budgets and fairness (so performance stays smooth)
- Per‑project budgets: max tokens/run, max $$/day, latency SLO.
- Scheduler honors budgets; Deep/Verified only when below budget or explicitly requested.
- Show live meters; degrade gracefully to Balanced/Quick when close to caps. See Cost & Quotas.

Caching and re‑use (speed without shortcuts)
- Semantic cache of Q→A with tight TTL; only reuse if same project, same policy, and high embedding similarity.
- Tool result cache with content hash and expiry (e.g., HTTP fetches, simple transforms).
- Never cache beyond project boundaries by default.

User‑visible guardrails that improve quality
- Context preview: what’s going in and why; token gauge; evidence diversity bar.
- Pre‑offload preview: what leaves the machine, to whom, estimated cost (ties into the Egress Ledger).
- Staging Area: risky actions queue with evidence; one‑click approve with a lease.
- One‑click “Deep” and “Verified” toggles per run with cost/latency hints.

Defaults to ship (so users get “best” without thinking)
- Balanced mode; self‑consistency auto‑on; k=12 retrieval; MMR on; diversity target ≥ 0.3.
- Abstain below 0.5 confidence; escalate 0.5–0.7; proceed ≥ 0.7.
- Evidence quota ≥ 50% of prompt tokens.
- Quick mode for chat‑like probes; Deep reserved for final synthesis.
- Verified for high‑risk outputs (external emails, orders, code that writes).

Feedback loop that actually converges
- After each run: log chosen strategy, confidence, tokens, latency, cost, tool success; verify claims spot‑check for research tasks.
- Nightly: retrain calibrators on goldens; prune low‑value memories; refresh top stale beliefs.
- Weekly: auto‑compare current vs last release on goldens; revert new Logic Units that lost.

What to explicitly avoid
- Blindly increasing context size to “be safe.”
- Always‑on ensembles; use gated self‑consistency instead.
- Silent remote offload; always show and meter.
- Uncalibrated confidence; it ruins abstention gates and UX trust.

Implementation status in ARW
- Shipping: budgets/admission on downloads; Egress Ledger format; Runtime Matrix skeleton; World Model scaffold; evaluation harness basics.
- In progress: richer Deep/Verified UX across all surfaces; MMR retrieval recipe; self-consistency gates; calibrated confidence training loop; per-project cost/token meters.
- Planned: offload runner with policy‑backed egress and leases; staging area approvals; semantic cache with TTL.

See also: Architecture → Budgets & Context, Scheduler & Governor, Context Working Set; Guide → Runtime Matrix, Evaluation Harness, METRICS & INSIGHTS, Cost & Quotas.
