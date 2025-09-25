---
title: Training Park
---

# Training Park
Updated: 2025-09-20
Type: How‑to

Status: **Telemetry live, UI stub.** `arw-server` now exposes `/state/training/telemetry` and a `training_metrics` read-model; the launcher window still renders placeholder controls until we wire the new data through.

The goal remains: a third primary perspective for tuning instincts, memory, and behavior without drowning in raw logs.

## What Ships Today

- Shared right-sidecar lanes (Timeline, Context, Policy, Metrics, Models) via the general SSE connection.
- `GET /state/training/telemetry` snapshot with route stats, tool success rate, and bus health, plus `state.read.model.patch` id `training_metrics` for live updates.
- Manual A/B button stub for future experiments (launcher still toast-only until UI work lands).
- Underlying metrics piggyback on the same collectors powering `/state/route_stats`, so telemetry remains consistent with other dashboards.

## Implementation Plan (`arw-server` + Launcher)

1. **Expand telemetry** — extend the current read-model with context assembly stats, memory coverage, and retriever diagnostics (`t-250918120201-tp01`).
2. **Expose controls** — model tunable presets (`ARW_CONTEXT_*`, `ARW_PERF_PRESET`) as structured actions so adjustments flow through `/actions` with policy/lease checks.
3. **Upgrade UI** — replace the launcher stub with live meters, sparklines, and controls bound to the telemetry + actions (`t-250918120205-tp02`).
4. **Record sessions** — append adjustments to the kernel so Training runs can promote configs into Logic Units or project hints with provenance.

## Inspect Telemetry

- Poll `GET /state/training/telemetry` for a JSON snapshot or stream the `state.read.model.patch` feed with id `training_metrics` for live updates.
- Key indicators: route latencies (`/context/assemble`, `/actions`), action success rate, and bus health—all emitted from the same telemetry endpoint powering the launcher view.

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
