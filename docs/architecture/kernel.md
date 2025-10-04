---
title: Kernel (SQLite Journal + CAS)
---

# Kernel (SQLite Journal + CAS)

The Kernel provides a single append-only event journal (SQLite/WAL) and content-addressed storage (CAS) co-located in the state
directory. All state views derive from the journal.

Updated: 2025-09-21
Type: Explanation

## Goals
- Single source of truth for events and actions
- Durable replay across restarts
- Portable artifacts via CAS (sha256)
- Backs the `/actions`, `/events`, and `/state/*` API in `apps/arw-server`

## Schema
- `events(id INTEGER PRIMARY KEY, time TEXT, kind TEXT, actor TEXT NULL, proj TEXT NULL, corr_id TEXT NULL, payload TEXT)`
- `artifacts(sha256 TEXT PRIMARY KEY, mime TEXT, bytes BLOB, meta TEXT)`
- `actions(id TEXT PRIMARY KEY, kind TEXT, input TEXT, policy_ctx TEXT NULL, idem_key TEXT NULL, state TEXT, created TEXT, updated TEXT)`

## API surface (current)
The unified server now ships the triad API backed by the kernel. The [Restructure Handbook](../RESTRUCTURE.md) tracks this work under “Kernel + Triad API complete in `arw-server` (now)” and calls out the same handlers listed below.

### Actions lifecycle (`apps/arw-server/src/api/actions.rs`)
- `POST /actions` — accepts a `kind`, JSON `input`, and optional `idem_key`, enforces queue depth and lease-aware policy checks, persists the request via `insert_action`, emits `actions.submitted`, and appends a `task.submit` contribution entry.
- `GET /actions/:id` — returns the stored action metadata, inputs, outputs, and timestamps directly from the kernel.
- `POST /actions/:id/state` — transitions an action between `queued`, `running`, `completed`, and `failed`, publishes the appropriate lifecycle event (`actions.running|completed|failed`), and records any supplied error context.

### Events stream (`apps/arw-server/src/api/events.rs`)
- `GET /events` — Server-Sent Events with optional `after`/`Last-Event-ID` resume, `replay=N` journal tailing, and `prefix` filters. The handler deduplicates recent envelopes via a digest cache and streams the bus while the kernel keeps the durable ledger.

### State views (`apps/arw-server/src/api/state.rs`)
- `GET /state/episodes` — groups the latest 1000 events by `corr_id`, returning per-episode metadata (start/end/last timestamps, duration, counts, error tally, first/last kinds, participating projects/actors) alongside the event payloads. Each event entry carries an `error` flag so clients can highlight failure steps without re-parsing payloads. The handler accepts `limit`, `project`, `errors_only`, `kind_prefix`, and `since` query parameters for focused timelines, while `GET /state/episode/{id}/snapshot` exposes a detailed view for a single correlation id.
- `GET /state/route_stats` — merges bus counters with the metrics snapshot to report publish/delivery counts and per-route histograms.
- `GET /state/actions` — paginated listing of persisted actions (default 200, configurable via `limit` query parameter).
- `GET /state/contributions` — returns the append-only contribution ledger (latest 200 entries by default).
- `GET /state/egress` — exposes recent egress ledger decisions when the ledger toggle is enabled.
- `GET /state/models` — serves the current `state/models.json` cache (with defaults when absent).
- `GET /state/self` and `GET /state/self/:agent` — enumerate and fetch `state/self/*.json` profiles for local agents.

### Egress controls (`apps/arw-server/src/api/egress_settings.rs`, `apps/arw-server/src/api/egress.rs`)
- `GET /state/egress/settings` — reports the effective posture, allowlist, proxy, DNS guard, and ledger flags sourced from environment/config state.
- `POST /egress/settings` — admin-gated patch that validates against `spec/schemas/egress_settings.json`, snapshots the new config through the kernel, emits `egress.settings.updated`, and reapplies proxy toggles.
- `POST /egress/preview` — evaluates a prospective network request against IP literal blocking, allowlists, and policy/lease gates, logging the decision to the egress ledger when enabled and returning an allow/deny verdict.

## Integration
- The in-process Bus dual-writes to the Kernel (subscribe + append). This preserves current interactive behavior (SSE) while enabling durable replay and aligns with the rollout tracked in the [Restructure Handbook](../RESTRUCTURE.md).
