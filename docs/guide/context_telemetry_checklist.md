---
title: Context Telemetry Checklist
---

# Context Telemetry Checklist
Updated: 2025-09-28
Type: How‑to

Use this checklist whenever you touch the context assembly loop, coverage heuristics, or the dashboards that depend on them. It keeps the `context.coverage` and `context.recall.risk` streams healthy and ensures downstream read-models expose slot budgets and counts for UI meters.

## Goals
- `context.coverage` events always ship the latest summary + spec snapshots, including `slots.counts` and `slots.budgets`.
- `context.recall.risk` events emit a populated `components.slots` map alongside the blended score and level.
- Training telemetry (`/state/training/telemetry`) captures the same data so the launcher and dashboards stay in sync.

## Checklist

- **Unit tests** — Run the focused assertions that guard the event payloads:
  ```bash
  cargo test -p arw-server \
    context_loop::tests::recall_risk_payload_combines_gaps \
    context_loop::tests::coverage_payload_captures_slot_budgets_and_metadata
  ```
  These tests fail if slot budgets, counts, or the component breakdown ever drop from the event payloads.

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
    jq '.context | {latest_verdict: .coverage.latest, top_slots: .coverage.top_slots, recall_rollup: .recall_risk}'
  ```
  Check that:
  - `coverage.latest.summary.slots.budgets` mirrors your request AND the new `coverage.top_slots` lists any `slot_underfilled:*` issues.
  - `recall_risk.top_slots` highlights the same gaps you saw on the event stream (averages and max gaps line up with raw events).

- **Metrics scrape** — Confirm slot metrics reach Prometheus-friendly gauges/counters:
  ```bash
  curl -sS http://127.0.0.1:8091/metrics | \
    rg 'arw_context_(slot_gap|slot_fill_ratio|slot_underfilled_total)'
  ```
  Expect to see `arw_context_slot_gap_latest{slot="instructions"}` (gauge), histogram samples under `arw_context_slot_gap_bucket`, and a monotonically increasing `arw_context_slot_underfilled_total` counter for slots that fell short.

- **Regression note** — If any check fails, halt the release, restore the missing fields, and add a unit test that captures the regression before re-running this list.

Keep this page handy in PR descriptions or release notes to document verification runs before enabling new context assembly behaviour.
