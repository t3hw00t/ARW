---
title: Restructure Handbook (Source of Truth)
---

# Restructure Handbook (Source of Truth)

This document is the single source of truth for the ongoing ARW restructure. It is written so a new contributor (or a chat without prior context) can pick up work immediately.

Updated: 2025-09-19
Type: Explanation
Owner: Core maintainers
Scope: Architecture, APIs, modules, migration plan, status, hand‑off tips

## Vision (Harmonized)
- Free, local‑first, privacy‑first agents that anyone can run on a laptop (CPU‑friendly), producing research‑grade output (provenance, coverage, verification, replayability).
- Agents learn and grow: adaptive memory + skills; safe autonomy via plans, simulation, and leases; explicit policies and mandatory egress firewall.
- Federation (opt‑in): connect peers/servers to co‑run/co‑train under policy; contributions tracked with fair value assignment; portable split contracts.

## Snappy UX (Performance Contracts)
Fast feedback is a product value. We design for immediacy:

- Budgets (targets):
  - Intent‑to‑first event (I2F) ≤ 50 ms
  - First partial response ≤ 150 ms
  - P95 route latency budgets per endpoint (see Guide → Interactive Performance)
- Streaming by default: `/events` is always on; `/actions` returns 202 quickly and progress streams over SSE.
- HTTP layers: compression, tracing, and a global concurrency governor (`ARW_HTTP_MAX_CONC`, default 1024) provide stable latency under load.
- Non‑blocking request paths: enqueue and return; heavy work runs in workers; avoid synchronous compute in handlers.
- Warm starts: pre‑warm caches (read‑models, prepared SQL, HTTP clients) at boot for low first‑hit latency.
- Small writes, big reads: journal writes are small and fast; large artifacts go to CAS; clients fetch head or stream on demand.
- Singleflight + caches: coalesce identical work; use short‑lived in‑mem caches and durable CAS for reuse.
- Bounded IO: cap inline file reads (e.g., 64 KB head); paginate views; chunk long operations.
- WAL + indexes: SQLite WAL mode, targeted indexes, prepared statements; avoid full‑table scans in hot paths.
- Backpressure: queue with fairness; reject/slow when budgets are exceeded rather than stalling the UI. Knob: `ARW_ACTIONS_QUEUE_MAX` (default `1024`).
 - Presets: seed sane defaults via `ARW_PERF_PRESET` (`eco|balanced|performance|turbo`) or auto‑detect. See How‑to → Performance Presets. Explicit env vars still override.

Implementation touchpoints in the new stack:
- `/actions`: 202 Accepted quickly, with `actions.submitted` event; worker lifecycle emits `actions.running/completed/failed`.
- `/events`: streaming SSE with keep‑alive; consumers reconnect and resume naturally.
- `/events?replay=N`: optional replay of the last N events from the journal before live streaming.
- Kernel: WAL, prepared statements, small per‑request transactions; content goes to CAS.
- Context: assemble is fast (small K), rehydrate is bounded (head bytes), both stream progress via events when necessary.
  - Implementation: `/context/assemble` offloads hybrid retrieval to blocking workers (for both streaming and synchronous responses) so the async runtime stays responsive; `/context/rehydrate` caps head bytes via `ARW_REHYDRATE_FILE_HEAD_KB`.
- Egress: proxy/ledger happen off the synchronous path; preview is an action with SSE.

Server modules (in progress)
- Router split for maintainability and clarity:
  - Extracted: `api_policy`, `api_events`, `api_context`, `api_actions`, `api_memory`, `api_connectors`, `api_state`, `api_config`, `api_logic_units`, `api_leases`, `api_orchestrator`.
  - Files:
    - Meta: `apps/arw-server/src/api_meta.rs`
    - Policy: `apps/arw-server/src/api_policy.rs`
    - Events: `apps/arw-server/src/api_events.rs`
    - Context: `apps/arw-server/src/api_context.rs`
    - Context Loop Driver: `apps/arw-server/src/context_loop.rs`
    - Actions: `apps/arw-server/src/api_actions.rs`
    - Memory: `apps/arw-server/src/api_memory.rs`
    - Connectors: `apps/arw-server/src/api_connectors.rs`
    - State Views: `apps/arw-server/src/api_state.rs`
    - Config Plane: `apps/arw-server/src/api_config.rs`
    - Logic Units: `apps/arw-server/src/api_logic_units.rs`
    - Leases: `apps/arw-server/src/api_leases.rs`
    - Orchestrator: `apps/arw-server/src/api_orchestrator.rs`
- Background loops:
    - Local Worker: `apps/arw-server/src/worker.rs` (lease gating + centralized egress ledger/event helper).
    - Read-model Publishers: `apps/arw-server/src/read_models.rs` (shared hashed patch scheduler across logic units, orchestrator jobs, memory, route stats).
- Helper utilities: `apps/arw-server/src/util.rs` (`default_models`, `effective_posture`, `state_dir`, `attach_memory_ptr`).
- Launcher packaging: `apps/arw-launcher/src-tauri/build.rs` (stages platform-specific binary variants expected by Tauri bundling).

See also: Guide → Interactive Performance and Interactive Bench.

## Infinite Context Window (Pragmatic Design)
We implement a practical “infinite context window” by treating context as an on‑demand working set, not a single static prompt. Key ingredients:

- Working Set Builder (WSB): hybrid retrieval over FTS5 + embeddings + graph relations to assemble the minimal, high‑value context for the current step.
- Stable Pointers + Rehydrate: every item carries a stable pointer (file, memory record, belief/claim, episode) so agents can rehydrate full content on demand (pull, don’t stuff).
- Diversity + Compression: MMR/diversity selection plus LLMLingua‑style compression to fit budgets while preserving signal.
- Streaming Assembly & Coverage: `/context/assemble` can stream `working_set.*` SSE events (`working_set.seed`, `working_set.expanded`, `working_set.selected`, `working_set.completed`) so clients render context as it lands, and a CRAG-style coverage loop widens lanes or relaxes thresholds before returning. This mirrors incremental retrieval patterns explored in [GraphRAG](https://arxiv.org/abs/2404.16130).
- Coverage-aware refinement: corrective loops look at the specific `coverage.reasons` (`below_target_limit`, `low_lane_diversity`, `weak_average_score`, etc.) and dial spec knobs accordingly (increase limit/expansion, widen lanes, lower thresholds) before the next pass. Each iteration surfaces the proposed `next_spec` so observers can see the planned adjustments before they run.
- Pluggable Scoring + Pseudo-Relevance Expansion: the working-set builder accepts scorer strategies (`mmrd`, `confidence`, or custom) and optional pseudo-relevance feedback that recomputes hybrid retrieval from the top seeds. The design keeps parity with the latest query expansion and scoring work while remaining tunable per project.
- Corrective Loop (CRAG): detect coverage gaps/hallucination risk, fetch additional evidence, and update the working set iteratively.
- Memory Lanes: episodic (raw traces), semantic (fact/claim graph), procedural (skills/templates). Long‑term context comes from lanes, not from long prompts.
- Multi‑modal By Default: text/code/files/images (OCR)/audio artifacts indexed and available via pointers.
- Safety Gates: redaction and guardrails before prompting; ABAC + leases for any rehydrate that touches sensitive files or network.

Effectively, the agent’s “context window” spans the entire indexed world, but the prompt is just the current working set. This yields recall and reproducibility without giant prompts.

## End‑State Architecture (One System)
- One Journal: SQLite/WAL event store + CAS blobs.
- One API: `/actions` (write), `/events` (SSE+replay), `/state/:view` (read).
- One Runtime: WASI plugins (perception/effectors/guardrails) with capability manifests.
- One Experience: unified UI (Memory Canvas, World Map, Influence Console) with a single right‑sidecar.

### Working Set Telemetry

- Metrics: the working-set builder reports `arw_context_phase_duration_ms`, `arw_context_seed_candidates_total`, `arw_context_query_expansion_total`, `arw_context_link_expansion_total`, `arw_context_selected_total`, and `arw_context_scorer_used_total` so operators can audit retrieval health and preset behavior. The dedicated driver in `apps/arw-server/src/context_loop.rs` adds `arw_context_iteration_duration_ms` (histogram) and `arw_context_iteration_total` (counter) tagged by `outcome` (`success|error|join_error`) and `needs_more` (`true|false`) so dashboards can track CRAG loop health and convergence speed.
- Streaming Diagnostics: SSE payloads include per-iteration summaries (`working_set.iteration.summary`) with coverage reasons, enabling dashboards to react to refinement loops in real time.
- Unified driver (`apps/arw-server/src/context_loop.rs`): `drive_context_loop` powers both synchronous responses and streaming SSE. `StreamIterationEmitter` forwards the same iteration payloads that land on the bus, while `SyncIterationCollector` records them for the final JSON body. Each summary payload now ships with `duration_ms` so clients can visualize per-iteration latency.
- Synchronous assembly still emits the same `working_set.iteration.summary` payloads on the unified bus, now with a `coverage` object (`needs_more`, `reasons`) and the exact spec snapshot for each iteration so non-streaming clients stay in lock-step with live dashboards.
- Iteration summaries include a `next_spec` snapshot whenever a follow-up iteration is scheduled, giving dashboards and optimizers a preview of the planned adjustments (lane set, limits, thresholds) that will power the next CRAG pass.
- Bus Events: every `working_set.*` emission lands on the main `GET /events` stream with `iteration`, `project`, `query`, and (when provided) `corr_id` metadata. Dashboards and the Project Hub sidecar no longer need a separate channel to follow context assembly progress.
- Shared iteration runner: both streaming and synchronous `/context/assemble` flows now call the same blocking worker wrapper in `context_loop.rs`, ensuring identical summary/error payloads while keeping heavy retrieval completely off the async runtime.

## Agent Orchestrator (Planned)
- Trains mini‑agents and coordinates agent teams under policy and budgets.
- Produces Logic Units from training episodes; integrates with the Evaluation Harness.
- Endpoints:
  - `GET /orchestrator/mini_agents` (list)
  - `POST /orchestrator/mini_agents/start_training` (admin; returns `{ job_id }` and emits `orchestrator.job.created`)
  - `GET /state/orchestrator/jobs` (job list with status/progress)
  - Events: `orchestrator.job.created`, `orchestrator.job.progress`, `orchestrator.job.completed`.
- Logic Unit suggestions: completed jobs emit `logic.unit.suggested` with a candidate manifest (stored in the kernel); evaluate then `POST /logic-units/apply` (dry‑run first) to stage.
- See: Architecture → Agent Orchestrator.

## Memory Abstraction Layer → Memory Overlay Service
- Canonical record now lives in `memory_items` with `{ id, ts, agent_id, project_id, kind, text, durability, trust, privacy, extra }` plus optional embeddings/links.
- Preferred interface (overlay):
  - `POST /actions (memory.upsert)` → emits `memory.item.upserted`
  - `POST /actions (memory.search)`
  - `POST /actions (memory.pack)` → journals decisions via `memory.pack.journaled`
  - `GET /state/memory` (JSON Patch stream of inserts/expirations/pack previews)
- Legacy REST (still wired through the new core while clients migrate):
  - `POST /memory/put`, `GET /state/memory/select`, `POST /memory/search_embed`, `POST /state/memory/select_hybrid`, `POST /memory/link`, `GET /state/memory/links`, `POST /memory/select_coherent`
- Purpose: stable centerpoint (self‑image and identity), dedupe via hashes, explainable hybrid retrieval (lexical + vector), and budget-aware context packs.
- See: Architecture → Memory Abstraction Layer, Memory Overlay Service, Memory Lifecycle.

## Connectors (Cloud & Local Apps)
- Purpose: let agents safely access cloud apps/storage (GitHub, Slack, Google/Microsoft 365, Notion, etc.) and local apps (VS Code, Word, Mail) through explicit, lease‑gated connectors.
- Registry: register connectors via `POST /connectors/register` with `{ id?, kind: cloud|local, provider, scopes[], meta? }`. List via `GET /state/connectors`.
- Tokens: update tokens via `POST /connectors/token` with `{ id, token?, refresh_token?, expires_at? }` (admin‑gated). Secrets are stored under `state/connectors/*.json`; `/state/connectors` redacts secrets.
- Events: `connectors.registered`, `connectors.token.updated`.
- Egress and policy: outbound calls still go through the egress policy (allowlists, leases). Connectors declare `scopes` and map to capability leases (e.g., `net:http:github.com`, `cloud:github:repo:rw`).
- Usage: pass `connector_id` in `net.http.get` input and the runtime injects `Authorization: Bearer <token>`. You can restrict domains per connector via `meta.allowed_hosts`.
- Local apps: access is mediated via dedicated tools (e.g., `app.vscode.open`, `app.word.open`) with tight leases (`io:app:vscode`, `io:app:word`) and no silent background automations.
  - Implemented: `app.vscode.open` (lease `io:app:vscode`) opens a path under `state/projects`; emits `apps.vscode.opened`.
- Security: no auto‑install; tokens set explicitly by user; redaction on state views; future: encryption-at-rest and hardware‑backed keyrings per OS.

## Unified Language & Design (One Voice)
- Event kinds: dot.case everywhere (code + docs). Run `scripts/lint_event_kinds.py`.
- API triad: only `/actions`, `/events`, `/state/*` for the unified server; avoid side channels.
- JSON shapes: lists under `items`, timestamps `created/updated` (RFC3339), stable keys `id/kind/state`.
- Responses: writes return `202 Accepted` with an id; progress over SSE; errors use RFC 7807 problem shape.
- Lexicon: Lease (capability grant), Policy (ABAC with leases), Egress Ledger (normalized network record), Working Set (context), Logic Unit (strategy pack), WASI Host (plugins), Decision (allow/require_capability/explain).
- Prose: US English, concise and friendly; headings in Title Case; keep consistent tone. See [Docs Style](developer/docs_style.md) and [CONTRIBUTING.md](https://github.com/t3hw00t/ARW/blob/main/CONTRIBUTING.md).
- Code: keep event/topic names and HTTP routes consistent with docs; prefer small, composable modules; avoid unnecessary renames during the restructure.
- Feature catalog: curate `interfaces/feature_catalog.json` alongside `interfaces/features.json`; run `python3 scripts/check_feature_integrity.py` then `python3 scripts/gen_feature_catalog.py` when capabilities move.

## New Modules (current status)
- `crates/arw-kernel` (SQLite + CAS) — Implemented
  - Tables: `events`, `actions`, `artifacts`, `contributions` (append‑only ledger).
  - APIs: `append_event`, `recent_events`, `insert_action`, `get_action`, `set_action_state`, `find_action_by_idem`, `append_contribution`, `list_contributions`.
  - File: `crates/arw-kernel/src/lib.rs`

- `apps/arw-server` (unified server) — In progress
  - `POST /actions`: idempotent queue; emits `actions.submitted`; appends contribution `task.submit`.
  - `GET /actions/:id`: returns action state and metadata.
  - `POST /actions/:id/state`: transitions (queued|running|completed|failed) and emits events.
  - `GET /events`: live SSE stream (bus); kernel dual‑writes events.
  - `GET /state/episodes`: groups recent events by `corr_id`.
  - `GET /state/route_stats`: combined bus counters, event kind totals, and per-route latency/hit metrics.
  - `GET /state/contributions`: contribution ledger snapshot.
  - File: `apps/arw-server/src/main.rs`

- Policy (facade + posture) — In progress
  - ABAC Facade: `arw-policy` crate provides a JSON‑backed policy engine with `allow_all` and `lease_rules` (kind_prefix → capability). `/actions` and context rehydrate are enforced via leases when required. See Guide → Policy (ABAC Facade).
  - Security Posture: `ARW_SECURITY_POSTURE=relaxed|standard|strict` selects a default policy when no `ARW_POLICY_FILE` is set. Default is `standard` (lease‑gates network, fs, rehydrate, app, browser, models download, shell).
  - Next: embed Cedar ABAC with an entity model (agents/projects/leases/capabilities) and explainers.
  - Egress: preview forward proxy and `/egress/preview` implemented; DNS guard (DoH/DoT) and IP‑literal guard enforced; `/state/egress/settings` for runtime toggles with schema `spec/schemas/egress_settings.json`.

- `crates/arw-wasi` (WASI runtime scaffold) — Implemented (skeleton)
  - Provides a `ToolHost` trait and a `NoopHost` implementation as a placeholder.
  - Future: host WASI Component plugins (tools) declared by WIT; enforce capability manifests and policy at spawn.

- Legacy bridge (status)
  - `apps/arw-svc` and its launch flags have been removed. Debug UI assets now live under `apps/arw-server/assets` and render when `ARW_DEBUG=1`.
  - Compatibility routes such as `/admin/events?replay=` remain available in `arw-server` for clients that still depend on them.
- Legacy feature migration (unified target — all todo unless noted)
  - Core services: port Model Steward (models download/CAS GC ✅), Tool Forge (tool runs/cache ✅), Feedback Loop, Experiment Deck, Memory Lanes, Project Hub primitives, Project Map read models, Snappy Governor, Event Spine patch streaming.
  - UI/experience: migrate Chat Workbench, Screenshot Pipeline, Self Card + forecasts to the new SPA/right-sidecar flow once endpoints land.
  - Policy & safety: unify Guardrail Gateway and Asimov Capsule Guard enforcement on `arw-server` (rely on upcoming policy/egress work) and remove launcher fallbacks to `/admin/*` once replacements ship.

### Legacy Feature Migration Track (runs parallel to phases 2–8)

| Phase | Focus | Features/Deliverables | Dependencies |
| --- | --- | --- | --- |
| A (Now) | Core services | Model Steward (models download/CAS GC ✅), Tool Forge (tool runs/cache metrics ✅), Snappy Governor (route stats view), Event Spine patch streaming | Triad kernel, metrics plumbing |
| B (Next) | Memory + projects | Memory Lanes (lane CRUD/save/load), Project Hub primitives (notes/files/patch), Project Map read models (observations/beliefs/intents) | Phase A storage, policy leases |
| C (Soon) | Feedback & experiments | Feedback Loop surfaces, Experiment Deck APIs, Self Card snapshots | Phase B data wiring |
| D (UI) | Operator experience | Chat Workbench, Screenshot Pipeline, launcher shift to SPA/right-sidecar, retire `/admin/*` debug windows | Phase A endpoints, UI unification groundwork |
| E (Safety) | Policy + guardrails | Guardrail Gateway on `arw-server`, Asimov Capsule Guard enforcement, final removal of legacy `/admin/*` shims | Policy & egress firewall phase |

Notes
- Phases can overlap if dependencies are satisfied; track owners and dates in the backlog so the matrix stays current.
- Update this section with status markers as phases complete (e.g., `A ✅`).
- Scripts: `scripts/start.{sh,ps1}` launch the unified server (launcher included). Use `ARW_NO_LAUNCHER=1` / `--service-only` for headless mode.
- Debug helpers `scripts/debug.{sh,ps1}` default to the unified stack and can open `/debug` when `ARW_DEBUG=1`.
  - Containers target `arw-server`; the legacy image is no longer published.

## Migration Plan (High‑level)
1) Kernel + Triad API complete in `arw-server` (now)
   - Actions lifecycle (done: submit/get/state)
   - SSE events (done)
   - State views (episodes/route_stats/contributions done; models/self next)
2) Policy & Leases (next)
   - Cedar ABAC; leases with TTL/scope/budgets; policy explainers at `/actions`
3) Egress firewall + DNS guard (next)
   - Loopback proxy + DNS allowlists; ledger entries; pre‑offload preview
   - Status: preview forward proxy and `/egress/preview` implemented; IP‑literal guard and ledger gating wired for `http.fetch` and proxy paths.
4) Runtime & WASI plugins
   - Host runtime; core perception/effectors/guardrails; schema‑driven forms
5) Orchestrator + Flow runner
   - Durable jobs; DAG flows (ReAct/Reflection/ToT); verifier branches; budgets
6) Memory & Learning
   - Lanes (episodic/semantic/procedural); CRAG consolidation; skill synthesis
7) Federation & Fair Value
   - Contribution ledger roll‑up; split capsules; negotiation flows; model cards with splits
8) UI Unification
   - SPA (Memory Canvas, World Map, Influence Console); retire legacy UI
9) Decommission legacy `arw-svc`

## What’s Implemented (Quick Index)
- Kernel + CAS: `crates/arw-kernel/src/lib.rs`
- New server (triad slice): `apps/arw-server/src/main.rs`
- SSE replay bridge (legacy): `apps/arw-svc/src/ext/mod.rs: triad_events_sse`
- Actions bridge (legacy): `apps/arw-svc/src/ext/actions_api.rs`
- Contribution ledger view: `GET /state/contributions` (new server)
 - Local worker (demo): dequeues queued → running → completed; appends `task.complete` to contributions.

## API Snapshot (new server)
- `POST /actions` → `{ id }` (202 Accepted)
- `GET /actions/:id` → `{ id, kind, state, input, created, updated }`
- `POST /actions/:id/state` → `{ ok: true }` (and event `actions.*`)
- `GET /events` → SSE (live bus; DB dual‑write)
- `GET /state/episodes` → `{ items: [{ id, events, start, end }] }`
- `GET /state/route_stats` → `{ bus: {…}, events: { start, total, kinds }, routes: { by_path: { "/path": { hits, errors, ewma_ms, p95_ms, max_ms } } } }`
- `GET /state/actions` → `{ items: [{ id, kind, state, created, updated }] }`
- `GET /state/contributions` → `{ items: [...] }`
- `GET /state/egress` → `{ items: [...] }`
- `GET /state/egress/settings` → effective egress posture and toggles
- `POST /egress/settings` → admin‑gated runtime update of egress toggles
- `POST /egress/preview` → `{ allow, reason?, host, port, protocol }` (applies allowlist, IP‑literal guard, and policy/lease rules; logs when ledger enabled)
- `GET /state/models` → `{ items: [...] }` (reads `state/models.json` or returns defaults)
- `GET /state/self` → `{ agents: [ ... ] }` (lists `state/self/*.json`)
- `GET /state/self/:agent` → the JSON content of `state/self/:agent.json`
 - `GET /about` → service metadata and discovery index; includes `endpoints[]` and `endpoints_meta[]` with `{ method, path, stability }` derived from in‑code path constants and route builders (avoids drift)
- `POST /leases` → `{ id, ttl_until }` (create lease; subject=`local`)
- `GET /state/leases` → `{ items: [...] }`
- `GET /state/policy` → `{ allow_all, lease_rules[] }`
- `POST /context/assemble` → assemble working set (hybrid memory retrieval; returns beliefs, seeds, and diagnostics; accepts optional `corr_id` to stitch events and publishes `working_set.*` on the main bus)
- `POST /context/rehydrate` → return full content head for a pointer (`file` head bytes or full `memory` record), gated by leases when policy requires

Events
- Egress ledger appends publish `egress.ledger.appended` with `{ id?, decision, reason?, dest_host?, dest_port?, protocol?, bytes_in?, bytes_out?, corr_id?, proj?, posture }`.
- Policy decisions emit `policy.decision` when an action is denied or lease‑gated (payload includes `action`, `allow`, `require_capability?`, and `explain`).
- SSE filters and resume: `/events?prefix=...` filters server‑side; `/events?replay=N` replays last N; `/events?after=<row_id>` replays after a journal id; honor `Last-Event-ID` as `after` when present.

Runtime
- Async Tool Host: `ToolHost` is async. `arw-wasi::LocalHost` implements `http.fetch`, `fs.patch`, and `app.vscode.open`. `http.fetch` enforces `ARW_NET_ALLOWLIST`, `ARW_HTTP_TIMEOUT_SECS`, and `ARW_HTTP_BODY_HEAD_KB`; appends to the egress ledger and emits events.
- `fs.patch` writes atomically under `ARW_STATE_DIR/projects` (or inside that root when given a relative path). Optional `pre_sha256` precondition prevents lost updates. Emits `projects.file.written`.

## Running Locally (new server)
```bash
cargo run -p arw-server
# Health
curl -s localhost:8091/healthz
# About / version / docs
curl -s localhost:8091/about | jq
# Submit an action
curl -s -X POST localhost:8091/actions -H 'content-type: application/json' \
  -d '{"kind":"demo.echo","input":{"msg":"hi"},"idem_key":"demo-1"}'
# Stream events
curl -N localhost:8091/events
# Views
curl -s localhost:8091/state/episodes | jq
curl -s localhost:8091/state/contributions | jq
curl -s localhost:8091/state/logic_units | jq
curl -s localhost:8091/state/orchestrator/jobs | jq
curl -s 'localhost:8091/state/memory/recent?limit=50' | jq
```

Notes
- The demo server binds to `127.0.0.1:8091` by default. Override with `ARW_BIND` and `ARW_PORT`.

## How To Try (End-to-End)

Environment and server
- Run: `ARW_STATE_DIR=state cargo run -p arw-server`

Policy (ABAC facade)
- Create `policy.json`:
  - `{ "allow_all": false, "lease_rules": [ { "kind_prefix": "net.http.", "capability": "net:http" }, { "kind_prefix": "context.rehydrate.memory", "capability": "context:rehydrate:memory" }, { "kind_prefix": "context.rehydrate", "capability": "context:rehydrate:file" } ] }`
- Export: `export ARW_POLICY_FILE=policy.json`
- Check: `curl -s localhost:8091/state/policy | jq`
- Simulate: `curl -s -X POST localhost:8091/policy/simulate -H 'content-type: application/json' -d '{"kind":"net.http.get"}' | jq`

Leases
- Create: `curl -s -X POST localhost:8091/leases -H 'content-type: application/json' -d '{"capability":"net:http","ttl_secs":600}' | jq`
- List: `curl -s localhost:8091/state/leases | jq`

Context
- Assemble: `curl -s -X POST localhost:8091/context/assemble -H 'content-type: application/json' -d '{"q":"term","lanes":["semantic","procedural"],"limit":18,"include_sources":true,"corr_id":"demo-ctx-1"}' | jq`
- Rehydrate file (lease‑gated): `curl -s -X POST localhost:8091/context/rehydrate -H 'content-type: application/json' -d '{"ptr":{"kind":"file","path":"state/projects/demo/notes.md"}}' | jq`
- Rehydrate memory (lease‑gated): `curl -s -X POST localhost:8091/context/rehydrate -H 'content-type: application/json' -d '{"ptr":{"kind":"memory","id":"<memory-id>"}}' | jq`  _(use `/state/memory/recent` to find ids)_

Actions and Events
- Submit: `curl -s -X POST localhost:8091/actions -H 'content-type: application/json' -d '{"kind":"net.http.get","input":{"url":"https://example.com"}}' | jq`
- Watch: `curl -N localhost:8091/events?replay=20`

Files (fs.patch)
- Require lease when policy is strict (example rule: kind_prefix `fs.` → capability `fs`).
- Create a lease: `curl -s -X POST localhost:8091/leases -H 'content-type: application/json' -d '{"capability":"fs","ttl_secs":600}' | jq`
- Write a file (atomic):
  - `curl -s -X POST localhost:8091/actions -H 'content-type: application/json' -d '{"kind":"fs.patch","input":{"path":"projects/demo/notes.md","content":"hello\n"}}' | jq`
- Watch `projects.file.written` on `/events`.

Network allowlist demo
- Restrict: `export ARW_NET_ALLOWLIST=example.com`
- Allowed: submit `net.http.get` to `https://example.com` → status and head bytes recorded; see `egress.ledger.appended`.
- Denied: submit to `https://google.com` → denied with reason `allowlist`; ledger records a `deny` decision.

State Views
- Egress: `curl -s localhost:8091/state/egress | jq`
- Actions: `curl -s localhost:8091/state/actions | jq`

## Contributor Checklist (Restructure)
- When adding/changing triad endpoints, kernel schemas, or runtime/policy:
  1. Update this file.
  2. Add or adjust `/state/*` views where applicable.
  3. Document flags/envs in [Configuration](CONFIGURATION.md).
  4. If it touches federation/economics, append to Contribution & Splits section.

## Next Milestones
- Cedar ABAC scaffold (entities, allow-default, explainers on `/actions`)
- WASI runtime host + first plugins (http.fetch, fs.patch, process.exec, guardrails.check)
- Egress proxy + DNS guard skeleton + ledger hooks
- Unified legacy capabilities on `arw-server` (Model Steward, Tool Forge, Snappy Governor, Event Spine patches, Feedback Loop, Experiment Deck, Memory Lanes, Project Hub/Map, Chat Workbench, Self Card, Screenshot Pipeline, Guardrail Gateway, Asimov Capsule Guard)
- Memory quarantine + world diff review queues now ship directly on `arw-server` (`/admin/state/memory/quarantine`, `/admin/state/world_diffs`).

## Logic Units (Continuous Updates)
- Strategy packs: Logic Units provide a safe way to adopt the latest research as config‑first bundles, with opt‑in code when necessary.
- Library: use the Logic Units Library to install, try (A/B), apply, and promote units with rollback and provenance.
- Patch Engine: config deltas are applied atomically with validation and audit. Emits `logic.unit.applied/reverted`.
- Manifests: see `spec/schemas/logic_unit_manifest.json`. Prefer config‑only units; code units must declare capabilities and are lease‑gated.
- Persistence: Logic Unit manifests are stored in the kernel; list via `GET /logic-units`.
- Server API (unified server, initial): `GET /logic-units`, `POST /logic-units/install`, `POST /logic-units/apply`, `POST /logic-units/revert`. These endpoints require `ARW_ADMIN_TOKEN` when set (`Authorization: Bearer` or `X-ARW-Admin`).
- Apply shape: `{ id, dry_run?, patches: [ { target, op: merge|set, value } ] }`. Revert shape: `{ snapshot_id }`.
- See also: Guide → Logic Units Library; Architecture → Config Plane & Patch Engine.

## Config Plane (Experimental)
- Effective config: `GET /state/config` returns the current merged config snapshot.
- Apply patches: `POST /patch/apply` accepts `{ id?, dry_run?, patches: [ { target, op: merge|set, value } ] }`.
  - Dry‑run: returns the projected config without persisting.
  - Apply: persists and snapshots in the kernel; emits `logic.unit.applied` (when `id` is provided) and `config.patch.applied`.
- Revert: `POST /patch/revert` with `{ snapshot_id }` restores a previous snapshot and emits `logic.unit.reverted`.
- Snapshots: list via `GET /state/config/snapshots?limit=50`; fetch via `GET /state/config/snapshots/:id`.
- Admin gating: `ARW_ADMIN_TOKEN` required for these endpoints when set (Authorization: Bearer or `X-ARW-Admin`).

Validation & Diffs
- Optional: include `schema_ref` (path to JSON Schema, e.g., `spec/schemas/recipe_manifest.json`) and `schema_pointer` (dot‑path into the final config) to validate the applied config or a sub‑tree.
- Response includes `diff_summary`: `{ target, pointer, op, before, after }[]` for each patch applied and a `json_patch` RFC‑6902 array suitable for preview UIs.
- Convenience: when `schema_ref` is not provided, the server attempts a best‑effort inference by mapping the first patch’s top‑level segment to known schemas (e.g., `recipes.*` → `spec/schemas/recipe_manifest.json`). Validation only runs when the schema file exists on disk.
- Validate only (no apply): `POST /patch/validate` with `{ schema_ref, schema_pointer?, config }` returns `{ ok: true }` or a 400 with error details.

Schema Registry (optional)
- Configure a schema map at `configs/schema_map.json` (or set `ARW_SCHEMA_MAP` to a file path) to declare per‑segment schemas and pointer prefixes. Example:
```json
{
  "recipes": { "schema_ref": "spec/schemas/recipe_manifest.json", "pointer_prefix": "recipes" },
  "policy":  { "schema_ref": "spec/schemas/policy_network_scopes.json", "pointer_prefix": "policy" }
}
```
- When present, the server uses this registry in preference to built‑in heuristics.
- Inspect map: `GET /state/schema_map` returns the active map (from `ARW_SCHEMA_MAP` or `configs/schema_map.json`).
- Infer mapping: `POST /patch/infer_schema` with `{ target: "recipes.default" }` returns `{ schema_ref, schema_pointer }` when a mapping is found.

## Why This Advances The Vision
- Snappy as a contract: 202 ASAP on `/actions`, streaming `/events` with optional `?replay`, bounded reads, caches, singleflight, WAL, and backpressure.
- Centralized, explainable policy: one `PolicyEngine` seam today; future Cedar swap with entity store and explainers without changing call sites.
- Accountable egress: normalized ledger with attribution and future pre‑offload previews out of the sync path.
- Practical infinite context: pointer‑based working set and lease‑gated rehydrate make large worlds usable without giant prompts.
