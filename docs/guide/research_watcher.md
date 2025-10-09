---
title: Research Watcher
---

# Research Watcher
Updated: 2025-10-09
Type: How‑to

Status: **Online ingestion (phase one).** `arw-server` polls configured feeds, seeds candidate Logic Units into the kernel, and serves `/state/research_watcher` snapshots plus approvals/archives through the unified API. Launcher Suggested tabs and the Debug UI consume the same read-model.

Use this guide to wire the watcher into your deployment and plan upcoming enhancements.

## Capabilities Today

- **Polling worker** — background task `research_watcher.poller` ingests JSON feeds or local seed files every `ARW_RESEARCH_WATCHER_INTERVAL_SECS` seconds (default 900, floor 300).
- **Kernel-backed catalog** — each item records `source`, `source_id`, `title`, `summary`, `url`, `status`, and optional payload metadata in CAS.
- **APIs & surfaces**
  - `GET /state/research_watcher` for snapshots; add `?status=pending` or `limit=200` as needed.
  - `state.read.model.patch` (id `research_watcher`) streams incremental updates to the launcher and sidecars.
  - `POST /research_watcher/{id}/approve|archive` updates status and emits `research_watcher.updated` events.
  - `/admin/debug` includes approve/archive controls; the launcher Suggested tab renders the same queue.
- **Event telemetry** — each ingest produces `research_watcher.updated` events with counts; status changes publish item-level updates.

## Configuration

- `ARW_RESEARCH_WATCHER_SEED`: optional local JSON file (`[ {...} ]` or `{ "items": [ ... ] }`) used at startup.
- `ARW_RESEARCH_WATCHER_FEEDS`: comma-separated HTTP(S) endpoints returning watcher payloads.
- `ARW_RESEARCH_WATCHER_INTERVAL_SECS`: polling cadence (minimum 300).

Example seed item:

```json
{
  "source": "arxiv",
  "source_id": "2409.01234",
  "title": "Agentic Retrieval Experiments",
  "summary": "Evaluates cascaded agent pipelines for retrieval-heavy tasks.",
  "url": "https://arxiv.org/abs/2409.01234"
}
```

## Operational Tips

- Keep feeds behind the egress proxy and DNS guard; watcher requests inherit the global network posture.
- Deduplicate via `source` + `source_id`; replays update in place without creating duplicates.
- Pair approvals with logic-unit promotion flows so the Suggested tab stays synchronized with what you ship.

## Roadmap

1. **Richer payloads** — convert RSS/HTML sources on ingest, add provenance previews, and store extended metadata for launcher cards (`t-250918120101-rw01`).
2. **Scoring & prioritisation** — add heuristics (recency, signal, workspace fit) and expose sort toggles in UI (`t-250918120105-rw02`).
3. **Library integration polish** — finalise Suggested tab UX with bulk actions, tags, and cross-install sharing (`t-250918120109-rw03`).

## Related Work

- Architecture: [architecture/logic_units.md](../architecture/logic_units.md)
- Reference: Logic Units Library (Suggested tab requirements)
