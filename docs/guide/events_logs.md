---
title: Events & Logs Windows
---

# Events & Logs Windows
Updated: 2025-09-20
Type: How‑to

## Events
- Prefix filter + presets (state/models/tools/egress/feedback)
- Include/Exclude body filters (substring match)
- Controls: Replay 50, Pretty JSON, Wrap, Pause, Clear, Copy last
- One SSE stream; capped buffer (300 entries)
- Watch capture activity by filtering `screenshots.`; the Activity lane/gallery subscribe to the same events.

## Logs (Introspection)
- Top routes table (hits, p95, ewma, max), route filter, SLO coloring
- Top event kinds table (by count)
- Focus tables mode to hide raw JSON
- Wrap toggle and Copy JSON available

Endpoints
- Events stream: `GET /events?prefix=...&replay=50`
- Introspection: `GET /admin/introspect/stats`
