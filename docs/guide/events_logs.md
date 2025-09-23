---
title: Events & Logs Windows
---

# Events & Logs Windows
Updated: 2025-09-21
Type: How‑to

## Events
- Prefix filter + presets (state/models/tools/egress/feedback)
- Include/Exclude body filters (substring match)
- Controls: Replay 50, Pretty JSON, Wrap, Pause, Clear, Copy last
- One SSE stream; capped buffer (300 entries)
- Read-model deltas stream over the same `/events` feed; use `prefix=state.read.model.patch` and supply `Last-Event-ID` to resume without losing JSON Patch diffs.
- Project metadata (`id="projects"`) now streams via patches so UIs can refresh notes and trees without polling.
- Watch capture activity by filtering `screenshots.`; the Activity lane/gallery subscribe to the same events.

## Logs (Introspection)
- Top routes table (hits, p95, ewma, max), route filter, SLO coloring
- Top event kinds table (by count)
- Focus tables mode to hide raw JSON
- Wrap toggle and Copy JSON available

- The canonical resume flow is `GET /events?prefix=state.read.model.patch&replay=50` followed by reconnects with the last `id` you observed.

Endpoints
- Events stream: `GET /events?prefix=...&replay=50`
- Introspection (deprecated; use `/state/route_stats` in new flows): `GET /admin/introspect/stats`
