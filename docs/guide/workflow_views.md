---
title: Workflow Views & Sidecar
---

# Workflow Views & Sidecar

ARW ships focused, modular workflow views — Project Hub, Chat, and Training Park — with a universal right‑sidecar that shows the same live state everywhere.

Updated: 2025-10-02
Type: How‑to

Key ideas
- One stream: a single SSE connection to `/events` powers all views.
- One graph: read‑models live under `/state/*` and publish JSON Patch deltas as `state.read.model.patch`.
- Universal sidecar: lanes for Timeline, Context, Policy, Metrics, Models, Activity, and the upcoming Provenance lane for modular stack validation/tool evidence summaries.

Windows
- Project Hub: Launcher → Windows → Project Hub
- Chat: Launcher → Windows → Chat
- Training Park: Launcher → Windows → Training Park

Training Park runs as a preview: telemetry feeds and launcher controls are live, and we are finishing richer visualizations and automation passes. See `guide/training_park.md` for the current status.

Compare
- Hub: Compare panel for Text/JSON (pretty + Only changes + Wrap + Copy), Image slider, and CSV/Table diff (key-based, Only changes, Copy summary, Export CSV). CSV export supports two modes: wide (after values only) and two-row (chg-before/chg-after rows).
- Hub → Runs: View a run snapshot, then use the Artifacts table to Pin A/B any output/payload to the Compare panel. Deep-link compare state with URL hash (cmpA/cmpB).
- Hub → Runs filters: Narrow the episodes table by actor, event kind, or free-text search across ids, projects, actors, and kinds. Use the Details toggle (keyboard accessible) to inspect start/end times, participants, and first/last events without leaving the table.
- Hub → Runs actions: View and Pin buttons expose descriptive ARIA labels so screen reader users hear the run or artifact context when navigating the table.
- Chat: Pin any message to A/B and run the same Text/JSON diff below; after image capture, consistent toolbar (Annotate, Copy MD, Save to project).
- Activity lane listens for `screenshots.captured`; thumbnails expose annotate/blur, copy Markdown, save-to-project, and open actions for recent captures.
- Provenance lane (rolling out with the modular cognitive stack) displays validation status badges, evidence links, and tool traces with screen-reader-friendly summaries and keyboard shortcuts mirroring the other lanes.
- Subscribe to `modular.agent.accepted` / `modular.tool.accepted` events when wiring the provenance lane so accepted responses and tool runs appear without scraping generic action logs.

SSE & Read‑models
- Subscribe with `GET /events?replay=25&prefix=state.` (or use the launcher Events window).
- Read‑models publish small JSON Patch deltas. Apply them locally to maintain a snapshot.
- Route stats stream as `state.read.model.patch` (id: `route_stats`) and are also available at `GET /state/route_stats`.
- Logic Units and Orchestrator Jobs also publish `state.read.model.patch` with ids `logic_units` and `orchestrator_jobs` (snapshots are available at `GET /logic-units` and `GET /state/orchestrator/jobs`).
- Memory recent snapshot publishes `state.read.model.patch` with id `memory_recent` (snapshot available at `GET /state/memory/recent`).
  - Snapshots include both `generated` and `generated_ms`—use the numeric timestamp for relative freshness and show the ISO value (or localised absolute time) alongside it in UI copy.
  - The `summary` node now carries `lanes` counts plus a `modular` object (`recent`, `pending_human_review`, `blocked`), making it easy to surface modular stack review queues without reprocessing the raw list.
- Modular review summary publishes `state.read.model.patch` with id `memory_modular_review` (snapshot available at `GET /state/memory/modular`) so lightweight dashboards can subscribe without replaying the full memory feed.
- Lane-specific snapshots publish as `state.read.model.patch` with ids like `memory_lane_short_term` (snapshot available at `GET /state/memory/lane/{lane}`) when you only need a single lane in the UI.

Policy & Context
- Policy lane reads `GET /state/policy` (active leases). Approvals surface here when enabled.
- Context lane lists top claims from the world model via `GET /state/world/select`.

Acceptance checks
- Sidecar shows the same Timeline/Models activity across Hub, Chat, and Training (single SSE).
- Metrics lane lists top routes with hits and P95, updating live.
- Policy lane shows leases when present; otherwise “No active leases”.
- Compare panel renders a diff for two pasted texts/JSON.

- Events window: prefix presets (state/models/tools/egress/feedback/rpu), include/exclude body filters, Pretty/Wrap/Pause, Replay 50.
- Debug UI is available at `/admin/debug` (set `ARW_DEBUG=1`). The launcher provides both browser and window shortcuts.
- For dot‑case event kinds (e.g., `models.download.progress`) older CamelCase listeners will not work.
