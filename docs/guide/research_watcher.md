---
title: Research Watcher
---

# Research Watcher
Updated: 2025-09-20
Type: How‑to

Status: **Planned.** No ingestion service or read-models ship in `arw-server` yet; the Launcher Library still shows static lists.

This page now tracks the rollout plan for the Research Watcher on the unified server.

## Objectives

- Surface candidate Logic Units from curated feeds without executing unknown code.
- Keep the queue auditable: sources, summaries, and approval history live in the kernel.
- Feed the Logic Units Library Suggested tab via `state.read.model.patch` so clients stay real-time.

## Implementation Plan (`arw-server`)

1. **Ingestion jobs** — add an async worker under `arw-server` that polls RSS/JSON feeds (arXiv, OpenReview, ACL, curated blogs) and records candidates as kernel jobs (`t-250918120101-rw01`). Use the existing job scheduler so retries, telemetry, and leases are consistent.
2. **Normalization pipeline** — map fetched items to a shared schema (title, gist, expected effect, compute profile, source URL). Store payloads in CAS to avoid duplicating blobs.
3. **Review queue read-model** — publish `/state/research_watcher` snapshots and `state.read.model.patch` deltas (`t-250918120105-rw02`). Reuse `read_models::publish_read_model_patch` for diffing and persistence.
4. **Approval endpoints** — expose minimal POST endpoints to approve/archive candidates, wiring decisions through the policy/lease system so audit trails land in the kernel.
5. **Launcher Library integration** — swap the Suggested tab to hit the new read-models and approval endpoints, keeping offline fallbacks (`t-250918120109-rw03`).

## Configure & Observe

- Seed suggestions locally with `ARW_RESEARCH_WATCHER_SEED` (path to JSON array) or point at remote feeds via `ARW_RESEARCH_WATCHER_FEEDS` (comma-separated HTTP(S) URLs returning `{ "items": [...] }`).
- Control polling cadence with `ARW_RESEARCH_WATCHER_INTERVAL_SECS` (defaults to 900 seconds, minimum 300).
- Inspect the live queue via `GET /state/research_watcher` or subscribe to `state.read.model.patch` with id `research_watcher`.

## Refactor & Optimization Notes

- Share the ingestion job harness with other future watchers (e.g., connector updates) to avoid bespoke schedulers.
- Lean on the existing CAS helpers for storing paper metadata; dedupe by stable IDs to avoid double-processing.
- Gate remote fetches behind the egress policy so feeds obey network posture settings.
- Keep summaries small and pre-computed; the Launcher should not re-render large markdown blobs for every SSE tick.

## Current Stopgap

Until the feed lands, populate Suggested units manually via the Library UI or commit sample manifests under `examples/logic-units/`.

## Related Work

- Backlog: `t-250918120101-rw01`, `t-250918120105-rw02`, `t-250918120109-rw03`
- Architecture: [architecture/logic_units.md](../architecture/logic_units.md)
- Reference: Logic Units Library (Suggested tab requirements)
