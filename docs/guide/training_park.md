---
title: Training Park
---

# Training Park
Updated: 2025-09-19
Type: How‑to

Status: **Prototype (stub).** The launcher window renders controls, but it emits toast placeholders and the service does not publish dedicated Training Park metrics yet.

The goal remains: a third primary perspective for tuning instincts, memory, and behavior without drowning in raw logs.

## What Ships Today

- Shared right-sidecar lanes (Timeline, Context, Policy, Metrics, Models) via the general SSE connection.
- Manual A/B button stub for future experiments.
- No dedicated `Training.*` or `Policy.*` events yet.

## Implementation Plan (`arw-server` + Launcher)

1. **Emit telemetry** — extend `arw-server` read-model loops to publish training metrics (`t-250918120201-tp01`). Start with context assembly stats, memory coverage, and tool success/failure counts; reuse `publish_read_model_patch` for diffing.
2. **Expose controls** — model tunable presets (`ARW_CONTEXT_*`, `ARW_PERF_PRESET`) as structured actions so adjustments flow through `/actions` with policy/lease checks.
3. **Upgrade UI** — replace the launcher stub with live meters, sparklines, and controls bound to the new read-model + actions (`t-250918120205-tp02`).
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

- Keep all adjustments lease-gated; risky changes still land in a staging queue once Human-in-the-loop approvals ship.
- Continue surfacing evidence previews when a change would offload work or rewrite policies.

## Related Work

- Backlog: `t-250918120201-tp01`, `t-250918120205-tp02`
- Backlog: `t-250912143033-0009` (original feature goals)
- See also: [guide/performance_presets.md](performance_presets.md), [guide/interactive_performance.md](interactive_performance.md)
