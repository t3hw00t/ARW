---
title: Logic Units
---

# Logic Units
Updated: 2025-10-09
Type: Explanation

Logic Units are installable, versioned strategy packs that change how an agent behaves without rewriting the app. Prefer config‑only units; code is optional and sandboxed.

What a Logic Unit is
- Identity: name, version, authors, license, provenance.
- Category & slots: which slot(s) it fills (Retrieval, Reasoning, Sampling, Policy, Memory, Evaluation) and compatibility constraints.
- Requirements: model families, min context window, capabilities, sandbox profile.
- Config patches: declarative edits to prompts, recipes, flows, policies, evaluators — with default values and tunable parameters.
- Metrics: expected effect, counters to show (e.g., solve‑rate, latency, cost, diversity).
- Safety: requested permissions (leases), audit flags, rollback notes.
- Tests: golden prompts, expected behaviors, A/B eval recipe.
- UI contribution: short explainer and a small control panel (knobs + readouts).

Packaging levels (choose the least‑powerful that works)
- Config‑only pack: prompts, recipes, flows, policy deltas, evaluator definitions. Zero code.
- Scripted transform: tiny sandboxed transform to compute config deltas from context (WASM/JS). No tools; strict capability gate.
- Tool‑bearing plugin: adds a tool/executor. Highest scrutiny; must declare capabilities and pass contract tests.

Object Graph placement
- First‑class entity: `LogicUnit` in the global inventory.
- Agent Profile exposes named slots; Projects/Agents bind units by slot.

Events (vocabulary)
- `logic.unit.suggested`, `logic.unit.installed`, `logic.unit.applied`, `logic.unit.reverted`, `logic.unit.promoted`.
  - Suggestions emitted by the orchestrator now include `hints.training` (mode/preset/diversity/recency/compression) and the logic unit patch already targets `governor.hints`, so applying the suggestion immediately aligns the runtime with the observed training run.

Good defaults to ship
- Metacognition: enable confidence estimates and simple calibration.
- Abstain/Escalate Gate: target risk–coverage and abstain when risk is high.
- Resource Forecaster: predict tokens/latency/$ for chosen recipes and feed budgets.
- Failure Modes Router: route fragile tasks (e.g., OCR tables, JS‑heavy sites) to safer plans.
- Never-Out-Of-Context: config-only unit that enforces slot budgets, diversity floor, and on-demand rehydration for context assembly (ships as `interfaces/logic_units/never_out_of_context.json`).
- Modular Stack Roster: declares which specialist agents (chat, recall, compression, validation, tooling) to activate per project and maps their leases/prompts to the [Modular Cognitive Stack](modular_cognitive_stack.md) contracts.

Endpoints (planned)
- `GET /logic-units` (list installed/experimental/suggested)
- `POST /logic-units/install` (config‑only by default)
- `POST /logic-units/apply` (target: project/agent; dry‑run=diff preview)
- `POST /logic-units/revert`
- `POST /logic-units/promote`
- `POST /logic-units/suggest` (curator input)

UI: Library & Composer
- Library tabs: Installed, Experimental, Suggested, Archived; filters (slot, model compat, risk, compute budget, effect size).
- Detail page: explainer, exact config diff, knobs, metrics panel, provenance links; actions: Apply, Try A/B.
- Composer: drag‑and‑drop to fill slots; compatibility checker warns on conflicts.

Governance & safety
- Default‑deny: only config‑only units auto‑install. Code units require signing, sandboxing, capability review, and contract tests.
- Reproducibility: episode snapshots record active units and versions for replay.
- Policy coupling: units cannot widen permissions silently; leases prompt inline.

Schemas
- See `spec/schemas/logic_unit_manifest.json` for the manifest.

See also: Modular Cognitive Stack, Evaluation Harness, Permissions & Policies, Events Vocabulary, Replay & Time Travel.
