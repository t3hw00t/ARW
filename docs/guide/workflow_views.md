---
title: Workflow Views & Sidecar
---

# Workflow Views & Sidecar

ARW ships focused, modular workflow views — Project Hub, Chat, and Training Park — with a universal right‑sidecar that shows the same live state everywhere.

Updated: 2025-09-15
Type: How‑to

Key ideas
- One stream: a single SSE connection to `/events` powers all views.
- One graph: read‑models live under `/state/*` and publish JSON Patch deltas as `state.read.model.patch`.
- Universal sidecar: lanes for Timeline, Context, Policy, Metrics, Models, and Activity.

Windows
- Project Hub: Launcher → Windows → Project Hub
- Chat: Launcher → Windows → Chat
- Training Park: Launcher → Windows → Training Park

Compare
- Hub: Compare panel for Text/JSON (pretty + Only changes + Wrap + Copy), Image slider, and CSV/Table diff (key‑based, Only changes, Copy summary, Export CSV). CSV export supports two modes: wide (after values only) and two‑row (chg‑before/chg‑after rows).
- Chat: Pin any message to A/B and run the same Text/JSON diff below; after image capture, consistent toolbar (Annotate, Copy MD, Save to project).
- Future: “Pin to compare” from runs/artifacts; side‑by‑side, timeline scrubber, and image slider.

SSE & Read‑models
- Subscribe with `GET /events?replay=25&prefix=state.` (or use the launcher Events window).
- Read‑models publish small JSON Patch deltas. Apply them locally to maintain a snapshot.
- Route stats stream as `state.read.model.patch` (id: `route_stats`) and are also available at `GET /state/route_stats`.
- Logic Units and Orchestrator Jobs also publish `state.read.model.patch` with ids `logic_units` and `orchestrator_jobs` (snapshots are available at `GET /logic-units` and `GET /state/orchestrator/jobs`).
 - Memory recent snapshot publishes `state.read.model.patch` with id `memory_recent` (snapshot available at `GET /state/memory/recent`).

Policy & Context
- Policy lane reads `GET /state/policy` (active leases). Approvals surface here when enabled.
- Context lane lists top claims from the world model via `GET /state/world/select`.

Acceptance checks
- Sidecar shows the same Timeline/Models activity across Hub, Chat, and Training (single SSE).
- Metrics lane lists top routes with hits and P95, updating live.
- Policy lane shows leases when present; otherwise “No active leases”.
- Compare panel renders a diff for two pasted texts/JSON.

- Events window: prefix presets (state/models/tools/egress/feedback/rpu), include/exclude body filters, Pretty/Wrap/Pause, Replay 50.
- Debug UI remains available at `/debug` (set `ARW_DEBUG=1`). The launcher provides both browser and window shortcuts.
- For dot‑case event kinds (e.g., `models.download.progress`) older CamelCase listeners will not work.
