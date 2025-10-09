---
title: Experiments (A/B) & Goldens
---

# Experiments (A/B) & Goldens

Updated: 2025-09-25
Type: How‑to

This guide covers running A/B experiments against project “goldens”, applying winners, and the core knobs you can tune without code.

## Goldens

Goldens are tiny, curated task sets per project with an automatic score. For chat‑like tasks, each item is `{id, kind: "chat", input: {prompt}, expect: {contains|equals|regex}}`.

Endpoints:

- List: `GET /admin/goldens/list?proj=NAME`
- Add: `POST /admin/goldens/add` → `{proj, id?, kind, input, expect}`
- Run: `POST /admin/goldens/run` → `{proj, limit?, temperature?, vote_k?}`

Events: `goldens.evaluated` with `{proj, total, passed, failed, avg_latency_ms}`.

## Experiments

Define experiments with named variants that carry configuration knobs. Each variant can set retrieval, formatting, and reasoning parameters.

Variant fields (all optional):

- Retrieval: `retrieval_k`, `mmr_lambda` (diversity via MMR)
- Reasoning: `vote_k`, `temperature`
- Compression: `compression_aggr` (0..1)
- Strict budget: `context_budget_tokens`, `context_item_budget_tokens`
- Context formatting: `context_format` (bullets|jsonl|inline|custom), `include_provenance`, `context_item_template`, `context_header`, `context_footer`, `joiner`

Endpoints:

- Define: `POST /admin/experiments/define` → `{id, name, variants: { "A": {...}, "B": {...} } }`
- Run A/B: `POST /admin/experiments/run` → `{id, proj, variants: ["A","B"]}`
- Activate: `POST /admin/experiments/activate` → `{id, variant}`
- List: `GET /admin/experiments/list`

### Scoreboard & Winners (Persistence)

The service persists the last known winners and a scoreboard snapshot per experiment under `state/experiments_state.json`.

Endpoints:

- `GET /admin/experiments/scoreboard` → `{ items: [ { exp_id, proj, variant, score: { passed, total, failed, avg_latency_ms, avg_ctx_tokens, avg_ctx_items, time } } ] }`
- `GET /admin/experiments/winners` → `{ items: [ { exp_id, proj, variant, time, passed, total, failed, avg_latency_ms, avg_ctx_tokens, avg_ctx_items } ] }`

Notes:

- Scoreboard rows are the most recent run snapshot per `(exp_id, variant)` (not cumulative).
- Winners are updated when `experiment.winner` is declared.

Events: `experiment.result` per variant; `experiment.winner` with top scorer.

## How A/B Runs Work

For each variant:

1. Retrieve context from the world model using `retrieval_k` and `mmr_lambda` (if set).
2. Apply optional proportional compression (`compression_aggr`).
3. Enforce a strict evidence token budget (`context_budget_tokens`), trimming long items and evicting tail items until the budget fits (per‑item cap).
4. Render context with the chosen format (bullets/jsonl/inline/custom).
5. Prepend the context to the prompt and run with self‑consistency (`vote_k`) and temperature.
6. Score against `expect` and aggregate solve rate and latency.

Additionally, the evaluator tracks average context footprint after packing:

- `avg_ctx_tokens` — avg tokens used by the evidence block (post‑pack)
- `avg_ctx_items` — avg selected items (post‑pack)

## Apply Winners

Activation copies the variant knobs into live hints so `POST /context/assemble` (and chat) use them immediately. This covers retrieval K, MMR lambda, compression, vote‑k, strict budgets, and formatting.

## Live Context Overrides & Preview

`POST /context/assemble` returns structured context and a `context_preview` string. Supply overrides in the JSON body (`{"q":"demo","limit":12,"slot_budgets":{"evidence":8}}`), and set `debug:true` to capture iteration diagnostics.

`aux.context` reports packing metrics: tokens/items before→after, budget, per‑item cap, and retrieval/λ.

## Debug UI (Experiments Panel)

Open `/admin` (Debug UI) and use:

- “Goldens / Experiments” box: seed goldens, run A/B, and see solve/latency.
- “Experiments (Live)” panel: list definitions; run A/B per experiment; activate winner; live scoreboard updates via SSE.
  - Badges show per‑variant solve rate, latency, and context KPIs: `ctx {tokens}t/{items}i`.
