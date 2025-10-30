---
title: Context Working Set (Never‑Out‑Of‑Context)
---

# Context Working Set (Never‑Out‑Of‑Context)
Updated: 2025-10-24
Type: Explanation

Core idea
- Keep only what the model needs right now in the prompt; keep everything else in structured memories you can fetch, compress, or rehydrate on demand.

Practical infinite context window
- Treat the “window” as an on‑demand working set, not a single prompt.
- Build the set via hybrid retrieval (FTS5 + embeddings + graph), apply MMR/diversity and token budgets.
- Attach stable pointers to everything; rehydrate to full content on demand.
- Use CRAG corrective loops and LLMLingua‑style compression to keep recall high within budgets.

Memory layers (each with its own budget + eviction)
- Working memory (hot, tiny): current user turn, live plan, tool I/O stubs, and a few key “registers” (instructions, constraints, budgets, safety).
- Episodic log (warm): compact summaries of past turns and actions with stable IDs that point back to full artifacts.
- Semantic memory (warm): vector/graph/KV indexes over notes, files, web grabs, code, and “beliefs” (claims with provenance and confidence).
- Procedural memory (warm): reusable flows/options (e.g., crawl->clean->index, triage->brief).
- Story threads (warm): topic-weighted threads linking episodic summaries so long-running efforts stay recallable without replaying every turn.
- Project world model (cool): small belief graph (entities, claims, constraints, open questions) with freshness and contradiction flags.
- Cold artifacts: full docs, transcripts, results—content‑addressed and never forced into the prompt unless rehydrated.

Context assembly (every turn)
- Plan first: start with a subgoal‑specific plan; choose the next small step.
- Targeted retrieval: build a small set from semantic + world memories using relevance, recency, and diversity (MMR‑style) to avoid duplicates.
- Token budgeter: fixed slots for instructions, plan, safety/policy, and evidence; leftover tokens go to nice-to-have context.
- Slot-aware assembly: `/context/assemble` accepts `slot_budgets` (map of slot -> max items). Selected items expose a normalized `slot` field—including the dedicated `story_thread` slot—and the response summarizes how many items landed in each slot so telemetry and UI can highlight gaps.
- Lane priorities: `/context/assemble` also honours `lane_priorities` (lane -> bonus weight). Values range from -1.0 to 1.0 and layer on top of the standard diversity bonus so personas or policies can gently tilt retrieval toward specific lanes without inflating the global limit. Seed defaults via `ARW_CONTEXT_LANE_PRIORITIES` using the same `{ "lane": weight }` JSON shape as slot budgets.
- Persona feedback: persona vibe telemetry is aggregated across runs and reshapes `lane_priorities`/`slot_budgets` each turn, layering the latest feedback on top of stored preferences so empathy signals directly steer retrieval.
- Capability tiers: the loop pulls a CapabilityService snapshot and applies lite/balanced/performance plans (limit ceilings, expansion defaults, iteration caps) so low-power hosts stay within safe budgets while workstation-class machines automatically widen the working set.
- Always include pointers: emit stable IDs alongside excerpts so the agent/UI can rehydrate more by ID when needed.
- Coverage-guided refinement: when `coverage.reasons` flag gaps (e.g., low lane diversity, weak scores, below target limit) the next iteration automatically widens lanes, increases expansion, or lowers thresholds before running. Dashboards see the proposed adjustments via the `next_spec` snapshot on each `working_set.iteration.summary` event.

Compression cascade (history never bloats)
- Extract -> Abstract -> Outline: turn long logs into key claims with sources, short rationales, then a skeletal outline that references artifacts.
- Rolling window: keep the last N raw tokens; summarize older chunks into the cascade; drop raw once linked from a summary.
- Entity rollups: merge repeated facts by entity with counters (mention frequency), recency, and confidence.

_Implementation status:_ `arw-server` ships a `context.cascade` background task that tails new episodes, waits for a brief cooldown, and writes the extract/abstract/outline bundle into the `episodic_summary` memory lane while forwarding topic hints to the `story_thread` lane. Each summary stores provenance (`sources.event_ids`) and the episode pointer so retrieval and rehydrate flows can jump back to the raw events. The launcher Training Park and Hub surfaces can now fetch these records without recomputing the cascade at render time. Capability-tier telemetry (`arw_capability_refresh_total` plus `working_set.iteration.summary.capability_tier`) keeps dashboards aware of which plan (lite/balanced/performance) shaped the current turn.

Never‑out‑of‑context controls
- Information‑gain gate: only admit a chunk if it reduces predicted error for the current subgoal (proxy: novelty × source reliability × task match).
- Diversity floor: keep a minimum fraction of context for other viewpoints to avoid tunnel vision.
- Rehydrate on demand: when detail is insufficient, ask for the next best slices by ID—never a blind dump.
- Abstain/escalate: if uncertainty stays high, abstain or trigger a targeted evidence‑gather step instead of stuffing more tokens.

World model assists
- Belief graph: bring 1–2 lines per belief with provenance, not pages.
- Open‑question queue: if the subgoal touches an unresolved question, schedule a short evidence‑gather step instead of hauling history.

Budgets and guarantees
- Hard slots: instructions, policy/safety, current plan, and evidence K have fixed token ceilings—so you never blow the window when a long file appears.
- Per‑layer caps: each memory layer enforces capacity + TTL (e.g., episodic summaries roll up weekly).
- Background hygiene: a tiny janitor keeps indexes fresh, dedups memories, and replaces low‑value long text with pointers + tighter summaries.

Failure detectors (fetch more when needed)
- Missed‑evidence heuristic: when answers cite sources not in prompt, raise a recall‑risk flag and re‑rank retrieval next turn.
- Coverage meters: show how many of the top‑K relevant chunks didn’t fit; if high, plan a short rehydrate step.
- Calibration: tie confidence to actual success on goldens; adjust admission thresholds (not token size) when miscalibrated.

When to use a long‑context model
- Rare merge steps (e.g., final synthesis across many sources). Offload to bigger context only for that step, then distill back into summaries/beliefs to return to small prompts immediately.

How this maps into ARW surfaces
- Context Recipes: formalize the pipeline (layers -> retrieval -> budgeter -> compression). See [Context Recipes](../guide/context_recipes.md).
- Training Park: dials for diversity, recency, compression aggressiveness; meters for recall risk and coverage.
- Project Hub: What’s in context now panel + pointers to the artifacts used.
- Logic Unit: Never-Out-Of-Context defaults ship as a built-in config-only unit so budgets, slot caps, diversity knobs, and rehydrate rules are seeded on boot. See [Logic Units](logic_units.md).
- World Model: use the Project Map belief graph for anchored facts and open‑questions; serve top‑K beliefs into assembly.

Why this works
- The model sees exactly what it needs for the step at hand.
- History is never thrown away—just compressed and referenced.
- Details are always recoverable via pointers and rehydration, so you don’t pay the token tax up front.
- The system stays explainable: every included line has provenance; every excluded chunk failed an explicit gate.

Implementation notes (ARW)
- Memory layers: existing lanes (`ephemeral`/`episodic`/`semantic`/`procedural`) in `MemoryService` will enforce per‑lane caps/TTL and emit `Memory.*` hygiene events.
- Context API: extend `POST /context/assemble` to accept slot budgets, diversity knobs, and to return pointers (stable IDs) for all included items.
- API snapshot now includes `slot_budgets` and per-slot counts/budgets inside `working_set.summary.slots`, enabling coverage checks (e.g., flagging `slot_underfilled:instructions`).
- Retrieval: add MMR‑style selector over vector/graph mounts and the world belief graph.
- Compression: background job to summarize episodes and roll up entities; write summaries to mounts with provenance.
- Cascade guardrails: the default sweep now caps `context.cascade` at ~2 K events per pass (512 per episode) so low-core hosts stay responsive. Bump via `ARW_CONTEXT_CASCADE_EVENT_LIMIT`, `ARW_CONTEXT_CASCADE_EPISODE_LIMIT`, or `ARW_CONTEXT_CASCADE_EPISODE_EVENT_LIMIT` when you need broader windows.
- Failure detectors: emit `context.recall.risk` and `context.coverage` events; surface in UI and adjust next‑turn retrieval.
  - `context.recall.risk` emits a normalized `score` (`0.0–1.0`), a coarse `level` (`low`/`medium`/`high`), and component gaps (`coverage_shortfall`, `lane_gap`, `slot_gap`, `quality_gap`) so downstream dashboards can highlight _why_ recall may be at risk. Each event carries the trimmed spec/summary snapshot plus project/query metadata for cross-linking back to the originating run.
    ```json
    {
      "kind": "context.recall.risk",
      "score": 0.74,
      "level": "high",
      "at_risk": true,
      "components": {
        "coverage_shortfall": 0.625,
        "lane_gap": 0.5,
        "slot_gap": 1.0,
        "slots": {
          "instructions": 1.0
        },
        "quality_gap": 0.8
      },
      "selected_ratio": 0.375,
      "spec": {
        "limit": 8,
        "lanes": ["analysis", "docs"],
        "slot_budgets": {
          "instructions": 2
        }
      },
      "summary": {
        "slots": {
          "counts": { "instructions": 0 },
          "budgets": { "instructions": 2 }
        }
      },
      "project": "alpha",
      "query": "how to seed"
    }
    ```
  - `context.coverage` keeps the full verdict + reasons so refinement loops can auto-adjust the next iteration while Training/Hub surfaces show the recent history.
    ```json
    {
      "kind": "context.coverage",
      "iteration": 0,
      "needs_more": true,
      "reasons": ["slot_underfilled:instructions", "below_target_limit"],
      "summary": {
        "slots": {
          "counts": { "instructions": 0 },
          "budgets": { "instructions": 2 }
        }
      },
      "spec": {
        "limit": 8,
        "slot_budgets": {
          "instructions": 2
        }
      },
      "project": "alpha",
      "query": "how to seed",
      "duration_ms": 150.0
    }
    ```
- The `context_metrics` read-model tails these events, delivering a ready-to-render payload (coverage verdicts, recall risk rollups, latest assembled snapshot) so SSE dashboards don’t need to replay the hourly journal window. It now aggregates slot gaps (`coverage.top_slots`, `recall_risk.top_slots`) so Training Park and other surfaces can call out which slots fall behind across iterations.
- Telemetry: publish `working_set.*` events (started/seed/expanded/selected/completed/iteration.summary/error) on the unified bus with `iteration`, `project`, `query`, and optional `corr_id` so `/events` listeners stay aligned with SSE streams. Every `working_set.iteration.summary` carries the spec snapshot plus a `coverage` object (`needs_more`, `reasons`) whether the client streamed or waited for the synchronous response.
- Iteration summaries append `next_spec` when a follow-up pass is queued so downstream planners can anticipate the new lane mix, limits, or scoring knobs before the next CRAG step begins.
- Long‑context: add an optional merge step in Recipes; distill back into beliefs/summaries after use.

See also
- Architecture -> Budgets & Context, Memory Lifecycle, World Model
- Guide -> Context Recipes, Training Park
