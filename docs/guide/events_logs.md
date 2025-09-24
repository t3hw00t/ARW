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
- Connection chip shows `connecting → on → retrying`; launcher reuses the last `id` to resume after transient drops.
- `Auto` pauses rendering only—SSE keeps the route stats model current so manual refresh renders the latest snapshot immediately.
- Read-model deltas stream over the same `/events` feed; use `prefix=state.read.model.patch` and supply `Last-Event-ID` to resume without losing JSON Patch diffs.
- Project metadata (`id="projects"`) now streams via patches so UIs can refresh notes and trees without polling.
- Watch capture activity by filtering `screenshots.`; the Activity lane/gallery subscribe to the same events.

## Logs (Introspection)
- Top routes table (hits, p95, ewma, max), route filter, SLO coloring
- Top event kinds table (by count)
- Focus tables mode to hide raw JSON
- Wrap toggle and Copy JSON available

- The canonical resume flow is `GET /events?prefix=state.read.model.patch&replay=50` followed by reconnects with the last `id` you observed.
- Desktop Hub walkthrough:
  1. Fetch `/state/projects` (and other `/state/*` read-model snapshots) before opening SSE so the UI has an immediate baseline.
  2. Connect to `/events?prefix=state.read.model.patch&replay=25` and cache the `id` from each event.
  3. Apply each patch where `payload.id` matches the model you care about (`projects`, `snappy`, `episodes`, etc.).
  4. On reconnect, send `Last-Event-ID` to receive any missed patches, keeping the UI fully consistent with the journal.

Endpoints
- Events stream: `GET /events?prefix=...&replay=50`
- Introspection (deprecated; use `/state/route_stats` in new flows): `GET /admin/introspect/stats`
