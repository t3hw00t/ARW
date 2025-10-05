---
title: Training Park
---

# Training Park
Updated: 2025-09-28
Type: How‑to

Status: **Telemetry and launcher controls are live; richer charts are still in flight.** `arw-server` now exposes `/state/training/telemetry` plus `training_metrics` and `context_metrics` read-models, and the launcher streams live metrics, job actions, and logic-unit history while we finish the advanced visualization pass.

The goal remains: a third primary perspective for tuning instincts, memory, and behavior without drowning in raw logs.

## What Ships Today

- Shared right-sidecar lanes (Timeline, Context, Policy, Metrics, Models) via the general SSE connection.
- `GET /state/training/telemetry` snapshot with route stats, tool success rate, cache/gov/capsule health, and bus metrics, plus `state.read.model.patch` ids `training_metrics` and `context_metrics` for live updates. The context portion now includes aggregate slot-gap analytics (`coverage.top_slots`, `recall_risk.top_slots`) so you can spot recurring under-filled slots without diffing raw events.
- Prometheus metrics (`arw_context_slot_gap`, `arw_context_slot_gap_latest`, `arw_context_slot_fill_ratio`, `arw_context_slot_underfilled_total`) persist slot-gap trends beyond the in-memory replay window so dashboards can chart regressions over longer horizons.
- Launcher controls submit training runs through `/orchestrator/mini_agents/start_training`, sending preset/diversity/compression hints and streaming job progress back into the results panel.
- Job table filters allow you to focus on running/completed/failed runs, while inline details expand to show payloads captured in `/state/orchestrator/jobs`. The kernel now emits canonical `status_slug` + `status_label` fields in that snapshot so launchers, CLIs, and scripts share the same vocabulary without bespoke mappings. Capsule telemetry mirrors this convention (`sample[].status_slug` + `sample[].status_label`) so countdown cards can reuse the same labels without local lookup tables.
- Each job exposes the suggested logic unit (if any) with buttons to dry-run patches, apply them, or hide the job locally once handled.
- Keyboard support: use ↑/↓ to move between jobs, `Shift+A` to apply, `Shift+D` to dry-run, and press Enter to toggle details. A “Recent logic unit actions” panel records the last 10 actions inline while `/state/training/actions` streams the longer history for export/download.
- Underlying metrics piggyback on the same collectors powering `/state/route_stats`, so telemetry remains consistent with other dashboards.

## Implementation Plan (`arw-server` + Launcher)

1. **Expand telemetry** — extend the current read-model with context assembly stats, memory coverage, and retriever diagnostics (`t-250918120201-tp01`). _Progress_: cache hit/miss, governor hints, capsule lease health, and feedback cues now ship in the snapshot; context/memory metrics remain.
2. **Expose controls** — model tunable presets (`ARW_CONTEXT_*`, `ARW_PERF_PRESET`) as structured actions so adjustments flow through `/actions` with policy/lease checks. _Status: live in the launcher; iterating on preset quality._
3. **Upgrade UI** — finish the richer charts, sparklines, and adjustment controls now that the launcher streams live telemetry (`t-250918120205-tp02`).
4. **Record sessions** — append adjustments to the kernel so Training runs can promote configs into Logic Units or project hints with provenance.

## Inspect Telemetry

- Poll `GET /state/training/telemetry` for a JSON snapshot or stream the `state.read.model.patch` feeds `training_metrics` and `context_metrics` for live updates.
- Prefer `arw-cli context telemetry --base http://127.0.0.1:8091 --watch` to stream quick terminal summaries of coverage/recall gaps, cache counters, and working-set scope without copying URLs (drop `--watch` for a single snapshot, append `--json --pretty` for raw output). Add `--output logs/trial-context.log` (or another path) to append each snapshot to a file for daily logs. Shortcut: `just context-watch` writes to `docs/ops/trials/logs/<DATE>/context.log` and keeps running until you press Ctrl+C.
- For cache instrumentation, run `arw-cli tools cache --base http://127.0.0.1:8091` to print hit/miss, stampede suppression, and latency/payload savings (append `--json` for raw stats that scripts can consume).
- Key indicators: route latencies (`/context/assemble`, `/actions`), action success rate, bus health, cache stampede suppression, governor profile/hints, capsule lease expirations, and feedback cues—all emitted from the same telemetry endpoint powering the launcher view.
- Context telemetry now streams `context.recall.risk` (score, level, component gaps) alongside the coverage verdicts so dashboards can surface why recall dipped without diffing raw events.
- Jobs panel pulls `/logic-units` alongside orchestrator state so operators can review/dry-run/apply the suggested patches without leaving the launcher.

### Prometheus & Grafana quickstart

- Scrape the service metrics endpoint (`/metrics`) and include the new gauges/histograms:
  - `arw_context_slot_gap_bucket`, `arw_context_slot_gap_latest` — per-slot recall gap distribution & latest gap.
  - `arw_context_slot_fill_ratio` — normalised coverage fill ratio per slot.
  - `arw_context_slot_underfilled_total` — counter of `slot_underfilled:*` reasons emitted by the coverage loop.
- Example Grafana query to chart the worst slot gap over time:
  ```promql
  max_over_time(arw_context_slot_gap_latest[15m])
  ```
- To break down by slot, group by the `slot` label (`topk(5, arw_context_slot_gap_latest)`); combine with the telemetry snapshot to link gaps back to specific projects and goals.

### Snapshot Fields (Current)

| Section | Fields |
| --- | --- |
| `events` | `start`, `total`, sorted `kinds` (topic → count). |
| `routes` | Array of `{path, hits, errors, ewma_ms, p95_ms, max_ms}` mirroring `/state/route_stats`. |
| `bus` | `published`, `delivered`, `receivers`, `lagged`, `no_receivers`. |
| `tools` | `completed`, `failed`, `total_runs`, `success_rate`. |
| `tasks` | Kernel task counters with last start/stop timestamps. |
| `cache` | Action cache stats (hit/miss/coalesced/errors/bypass, TTL, capacity). |
| `governor` | Active profile, optional memory limit, non-null hints. |
| `capsules` | `count`, `expiring_soon` (≤5m), `expired`, `sample` (sanitized view with id/version/lease fields, max 5 items). |
| `feedback` | `auto_apply`, signal count + recent five, suggestion count + three-sample. |
| `compatibility` | Legacy gauges (e.g., capsule header sightings). |
| `context` | Latest coverage verdict (needs_more, reasons, summary/spec), aggregated slot-gap analytics (`top_slots` identifies recurring `slot_underfilled:*` reasons), recall-risk rollups (score, level distribution, average/max slot gaps), and the most recent assembled snapshot (counts + final spec). Mirrors the `context_metrics` read-model so SSE dashboards and the launcher stay aligned without recomputing the journal replay. |

## Optimization & Refactor Notes

- Feed the Training Park from the same metrics powering `/state/route_stats` to avoid duplicating collectors.
- Cache computed aggregates (recall risk, coverage) in the kernel with TTL to reduce on-demand recomputation for every SSE subscriber.
- When the UI submits adjustments, prefer diff-style patches so repeated toggles do not thrash the job scheduler.
- Tie into the existing Action Cache once the A/B harness moves to `arw-server`, keeping repeated evaluation runs inexpensive.

## Safety

- Keep all adjustments lease-gated; risky changes land in the Human-in-the-loop staging queue before execution.
- Continue surfacing evidence previews when a change would offload work or rewrite policies.

## Related Work

- Backlog: `t-250918120201-tp01`, `t-250918120205-tp02`
- Backlog: `t-250912143033-0009` (original feature goals)
- See also: [guide/performance_presets.md](performance_presets.md), [guide/interactive_performance.md](interactive_performance.md)
