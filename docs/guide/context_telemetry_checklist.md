---
title: Context Telemetry Checklist
---

# Context Telemetry Checklist
Updated: 2025-10-24
Type: How‑to

Use this checklist whenever you touch the context assembly loop, coverage heuristics, or the dashboards that depend on them. It keeps the `context.coverage` and `context.recall.risk` streams healthy and ensures downstream read-models expose slot budgets and counts for UI meters.

## Goals
- `context.coverage` events always ship the latest summary + spec snapshots, including `slots.counts` and `slots.budgets`.
- `context.recall.risk` events emit a populated `components.slots` map alongside the blended score and level.
- Training telemetry (`/state/training/telemetry`) captures the same data so the launcher and dashboards stay in sync.
- Training telemetry also exposes `context.assembly`, `context.retriever`, and `memory` blocks with lane/slot aggregates and timing summaries for deeper triage.

## Checklist

- **Unit tests** — Run the focused assertions that guard the event payloads:
  ```bash
  cargo test -p arw-server -- \
    --test-threads=1 \
    context_loop::tests::recall_risk_payload_combines_gaps \
    context_loop::tests::coverage_payload_captures_slot_budgets_and_metadata
  ```
  These tests fail if slot budgets, counts, or the component breakdown ever drop from the event payloads.
- **Live watch** — Keep `arw-cli context telemetry --watch --base http://127.0.0.1:8091` running in a terminal while iterating. The CLI prints the same coverage/recall rollups the launcher uses, so you can see slot-gap regressions or telemetry failures as soon as they land. Use `--output logs/context_watch.log` (or another path) to append each snapshot for later review, or run `just context-watch` to drop them under `docs/ops/trials/logs/<DATE>/` automatically. Set `ARW_CONTEXT_WATCH_BASE`, `ARW_CONTEXT_WATCH_OUTPUT_ROOT`, or `ARW_CONTEXT_WATCH_SESSION` if you need different defaults without editing tooling; `ops/context_watch.env.example` includes ready-to-source exports. Pass `--date YYYY-MM-DD` / `--session <slug>` to backfill or split logs for prior sessions without manual edits.

- **Assemble with slot budgets** — Start `arw-server` (`bash scripts/start.sh --service-only --wait-health`) and run a request that exercises slot-aware retrieval:
  ```bash
  curl -sS -X POST http://127.0.0.1:8091/context/assemble \
    -H 'content-type: application/json' \
    -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
    -d '{
          "q": "seed doc",
          "lanes": ["semantic", "procedural"],
          "limit": 8,
          "slot_budgets": {"instructions": 2},
          "debug": true,
          "corr_id": "ctx-check"
        }' | jq '.working_set.summary.slots'
  ```
  Confirm the response mirrors the slot counts/budgets you expect.

- **Stream telemetry** — Tail both topics and ensure every event carries slot metadata and project/query context:
  ```bash
  curl -N \
    -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
    "http://127.0.0.1:8091/events?prefix=context.coverage&prefix=context.recall.risk&replay=3" \
  | jq '{kind, needs_more, reasons, components, slot_counts: .summary.slots.counts, slot_budgets: .summary.slots.budgets}'
  ```
  Expect to see `slot_budgets.instructions` (or other slots) populated in both the coverage and recall-risk payloads.

- **Read-model snapshot** — Verify the training telemetry surfaces the same data for dashboards:
  ```bash
  curl -sS -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
    http://127.0.0.1:8091/state/training/telemetry | \
    jq '{coverage: {latest: .context.coverage.latest, top_slots: .context.coverage.top_slots}, recall: .context.recall_risk, assembly: .context.assembly, retriever: .context.retriever, memory: .memory}'
  ```
  Check that:
  - `coverage.latest.summary.slots.budgets` mirrors your request AND the new `coverage.top_slots` lists any `slot_underfilled:*` issues.
  - `recall_risk.top_slots` highlights the same gaps you saw on the event stream (averages and max gaps line up with raw events).
  - `assembly.needs_more_ratio`, `assembly.lanes`, and `assembly.metrics` reflect the same slot budgets and iteration outcomes seen on the live stream.
  - `retriever.timings_ms` and lane/slot aggregates align with the `working_set.*` events you inspected.
  - The top-level `memory.lanes` totals and modular counters match the recent records in `/state/memory/recent`.

- **Metrics scrape** — Confirm slot metrics reach Prometheus-friendly gauges/counters:
  ```bash
  curl -sS http://127.0.0.1:8091/metrics | \
    rg 'arw_context_(slot_gap|slot_fill_ratio|slot_underfilled_total)'
  ```
  Expect to see `arw_context_slot_gap_latest{slot="instructions"}` (gauge), histogram samples under `arw_context_slot_gap_bucket`, and a monotonically increasing `arw_context_slot_underfilled_total` counter for slots that fell short.

- **Regression note** — If any check fails, halt the release, restore the missing fields, and add a unit test that captures the regression before re-running this list.

Keep this page handy in PR descriptions or release notes to document verification runs before enabling new context assembly behaviour.
