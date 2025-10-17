---
title: Restructure Handbook (Source of Truth)
---

# Restructure Handbook (Source of Truth)

This document is the single source of truth for the ARW restructure. It now serves as the hand-off reference for the completed migration and gives new contributors (or a chat without prior context) the context needed to work with the unified stack.

Updated: 2025-10-18
Type: Explanation
Owner: Core maintainers
Scope: Architecture, APIs, modules, migration plan, status, hand‑off tips

## How to Use This Handbook
- Start with the track summary below to understand how the restructure landed, then drill into the detailed tracks that follow.
- Status callouts pair an emoji with plain language so screen readers and reports capture the same signal.
- Cross-links point to source files or guides; keep those updated when you land a change to avoid drift.

## Status Legend
- ✅ Completed — work is merged and available in `main`.
- 🔄 In progress — actively being delivered; expect updates soon.
- ⏭ Next up — queued immediately after the active work wraps.
- ⏳ Planned — scoped but waiting on prerequisites or staffing.

## Alignment Pillars
- **Stability:** Prefer incremental migrations that keep `/actions`, `/events`, and `/state/*` reliable at every commit; expand regression coverage before removing shims.
- **Accessibility:** Mirror status text alongside icons, keep docs in plain language, and retain links to UI entry points so screen readers and handoffs stay frictionless.
- **Harmony:** Keep terminology synchronized between code, docs, and telemetry; when you rename a surface, update the style guide and topics tables together.
- **Optimization:** Fold in performance or ergonomics wins when touching a module—capture learnings in the Snappy UX contracts and retire redundant paths.

## Track Summary (Final)

| Track | Tracking Label(s) | Status | Check-in Cadence | Exit Criteria |
| --- | --- | --- | --- | --- |
| Unified server (`apps/arw-server`) | `proj:restructure/core`, `track:unified-server` | ✅ Completed | Weekly architecture sync (Wed) | Router module map current, background loops documented, and state/memory integrations landed without legacy regressions (2025-09-24) |
| Policy & posture | `proj:restructure/core`, `track:policy-posture` | ✅ Completed | Bi-weekly policy review | Cedar ABAC embed merged with explainers; posture presets keep capsules/leases/egress ledger aligned; hand-off run 2025-09-25 |
| Operator experience (Phase D) | `proj:restructure/phase-d`, `area:operator-experience` | ✅ Completed | Launcher debug stand-up (Fri) | Chat Workbench REST handlers, screenshot pipeline, SPA/right-sidecar swap, and `/admin/*` fallback removal verified 2025-09-25 |
| Safety (Phase E) | `proj:restructure/phase-e`, `area:safety` | ✅ Completed | Monthly safety council | Guardrail Gateway enforcement + Asimov Capsule Guard parity shipped; compatibility fallbacks retired after legacy traffic reached zero (2025-09-26) |

> Tracking notes: each row corresponds to an item in the “ARW Restructure” GitHub Project. Apply the listed labels to issues/PRs so automation keeps this table in sync with board status and dashboards.

All restructure workstreams are now closed. Maintain this table as a historical snapshot; future evolution for these areas lives in the standard backlog and roadmap streams.

## Vision (Harmonized)
- Free, local‑first, privacy‑first agents that anyone can run on a laptop (CPU‑friendly), producing research‑grade output (provenance, coverage, verification, replayability).
- Agents learn and grow: adaptive memory + skills; safe autonomy via plans, simulation, and leases; explicit policies and mandatory egress firewall.
- Federation (opt‑in): connect peers/servers to co‑run/co‑train under policy; contributions tracked with fair value assignment; portable split contracts.
- Empathetic, persona-aware interfaces: agents reflect user worldview and tone with transparent self-models, consented empathy signals, and inclusive defaults.
- Open autonomous economy: revenue workflows stay local-first, auditable, and fair so anyone can earn with collaborative agents.

## Snappy UX (Performance Contracts)
Fast feedback is a product value. We design for immediacy:

- Budgets (targets):
  - Intent‑to‑first event (I2F) ≤ 50 ms
  - First partial response ≤ 150 ms
  - P95 route latency budgets per endpoint (see Guide → Interactive Performance)
- Streaming by default: `/events` is always on; `/actions` returns 202 quickly and progress streams over SSE.
- HTTP layers: compression, tracing, and a global concurrency governor (`ARW_HTTP_MAX_CONC`) provide stable latency under load; presets seed the limit (256 → 16384) so lightweight hosts stay snappy and workstations scale.
- Non‑blocking request paths: enqueue and return; heavy work runs in workers; avoid synchronous compute in handlers.
- Worker pool auto-scales with host parallelism (≈2× cores by default, capped at 32 unless you set `ARW_WORKERS_MAX`) and can be pinned via `ARW_WORKERS` so long-running actions never starve the queue.
- Warm starts: pre‑warm caches (read‑models, prepared SQL, HTTP clients) at boot for low first‑hit latency.
- Small writes, big reads: journal writes are small and fast; large artifacts go to CAS; clients fetch head or stream on demand.
- Singleflight + caches: coalesce identical work; use short‑lived in‑mem caches and durable CAS for reuse.
- Bounded IO: cap inline file reads (e.g., 64 KB head); paginate views; chunk long operations.
- WAL + indexes: SQLite WAL mode, targeted indexes, prepared statements; avoid full‑table scans in hot paths.
- Backpressure: queue with fairness; reject/slow when budgets are exceeded rather than stalling the UI. Knob: `ARW_ACTIONS_QUEUE_MAX`, seeded by the preset tier alongside concurrency.
 - Presets: seed sane defaults via `ARW_PERF_PRESET` (`eco|balanced|performance|turbo`) or auto‑detect. See How‑to → Performance Presets. Explicit env vars still override.

Implementation touchpoints in the new stack:
- `/actions`: 202 Accepted quickly, with `actions.submitted` event; worker lifecycle emits `actions.running/completed/failed`.
- `/events`: streaming SSE with keep‑alive; consumers reconnect and resume naturally.
- `/events?replay=N`: optional replay of the last N events from the journal before live streaming.
- Kernel: WAL, prepared statements, small per‑request transactions; content goes to CAS.
- Context: assemble is fast (small K), rehydrate is bounded (head bytes), both stream progress via events when necessary.
  - Implementation: `/context/assemble` offloads hybrid retrieval to blocking workers (for both streaming and synchronous responses) so the async runtime stays responsive; `/context/rehydrate` caps head bytes via `ARW_REHYDRATE_FILE_HEAD_KB`.
- Egress: proxy/ledger happen off the synchronous path; preview is an action with SSE.

Server modules (post-migration snapshot)
- Router split for maintainability and clarity:
  - Extracted: `api_policy`, `api_events`, `api_context`, `api_actions`, `api_memory`, `api_connectors`, `api_state`, `api_config`, `api_logic_units`, `api_leases`, `api_orchestrator`.
  - Files:
    - Meta: `apps/arw-server/src/api/meta.rs`
    - Policy: `apps/arw-server/src/api/policy.rs`
    - Events: `apps/arw-server/src/api/events.rs`
    - Context: `apps/arw-server/src/api/context.rs`
    - Context Loop Driver: `apps/arw-server/src/context_loop.rs`
    - Actions: `apps/arw-server/src/api/actions.rs`
    - Memory: `apps/arw-server/src/api/memory.rs`
    - Connectors: `apps/arw-server/src/api/connectors.rs`
    - State Views: `apps/arw-server/src/api/state.rs`
    - Config Plane: `apps/arw-server/src/api/config.rs`
    - Logic Units: `apps/arw-server/src/api/logic_units.rs`
    - Leases: `apps/arw-server/src/api/leases.rs`
    - Orchestrator: `apps/arw-server/src/api/orchestrator.rs`
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
- `GET /state/memory` — SSE stream of snapshots + JSON Patch events (inserts/expirations/pack previews)
- Legacy REST (still wired through the new core while clients migrate):
- ✅ Legacy `/memory/*` REST shims (put/search_embed/select/link/coherent) removed — rely on `/actions` (`memory.*`) and `GET /state/memory/recent`.
- Purpose: stable centerpoint (self‑image and identity), dedupe via hashes, explainable hybrid retrieval (lexical + vector), and budget-aware context packs.
- See: Architecture → Memory Abstraction Layer, Memory Overlay Service, Memory Lifecycle.

## Connectors (Cloud & Local Apps)
- Purpose: let agents safely access cloud apps/storage (GitHub, Slack, Google/Microsoft 365, Notion, etc.) and local apps (VS Code, Word, Mail) through explicit, lease‑gated connectors.
- Registry: register connectors via `POST /connectors/register` with `{ id?, kind: cloud|local, provider, scopes[], meta? }`. List via `GET /state/connectors`.
- Tokens: update tokens via `POST /connectors/token` with `{ id, token?, refresh_token?, expires_at? }` (admin‑gated). Secrets are stored under `state/connectors/*.json`; `/state/connectors` redacts secrets. Connector scopes now require matching leases (e.g., grant `cloud:github:repo:rw` before using a GitHub connector) — missing scopes return `connector lease required` and emit `policy.decision`.
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
- Code: keep event/topic names and HTTP routes consistent with docs; prefer small, composable modules; avoid unnecessary renames when touching these historical restructure artifacts so diff history stays legible.
- Feature catalog: curate `interfaces/feature_catalog.json` alongside `interfaces/features.json`; run `python3 scripts/check_feature_integrity.py` then `python3 scripts/gen_feature_catalog.py` when capabilities move.

## New Modules (current status)
- `crates/arw-kernel` (SQLite + CAS) — Implemented
  - Tables: `events`, `actions`, `artifacts`, `contributions` (append‑only ledger).
  - APIs: `append_event`, `recent_events`, `insert_action`, `get_action`, `set_action_state`, `find_action_by_idem`, `append_contribution`, `list_contributions`.
  - File: `crates/arw-kernel/src/lib.rs`

- `apps/arw-server` (unified server) — Completed (2025-09-24)
  - `POST /actions`: idempotent queue; emits `actions.submitted`; appends contribution `task.submit`.
  - `GET /actions/:id`: returns action state and metadata.
  - `POST /actions/:id/state`: transitions (queued|running|completed|failed) and emits events.
  - `GET /events`: live SSE stream (bus); kernel dual-writes events.
  - `GET /state/episodes`: groups recent events by `corr_id`.
  - `GET /state/route_stats`: combined bus counters, event kind totals, and per-route latency/hit metrics.
  - `GET /state/contributions`: contribution ledger snapshot.
  - File: `apps/arw-server/src/main.rs`

### Policy (facade + posture)
Status: Completed (ongoing refinements flow through the security backlog).

- [x] ABAC facade (`arw-policy` crate) ships with JSON-backed rules, `allow_all`, and `lease_rules`; `/actions` and context rehydrate honor leases. See Guide → Policy (ABAC Facade).
- [x] Security posture defaults (`ARW_SECURITY_POSTURE=relaxed|standard|strict`) select guard presets when `ARW_POLICY_FILE` is absent; `standard` lease-gates network, fs, rehydrate, apps, browser, models download, and shell access.
- [x] Embed Cedar ABAC with entity modeling (agents/projects/leases/capabilities) and human-readable explainers.
- [x] Extend egress integration so posture presets, capsule leases, and ledger toggles move together (forward proxy, `/egress/preview`, DNS/IP guards already live; expose remaining controls via `/state/egress/settings`).
  - `/state/egress/settings` now reports `recommended` posture defaults alongside capsule and lease snapshots; admin updates inherit ledger/proxy defaults when posture changes.

- `crates/arw-wasi` (WASI runtime scaffold) — Implemented (skeleton)
  - Provides a `ToolHost` trait and a `NoopHost` implementation as a placeholder.
  - Future: host WASI Component plugins (tools) declared by WIT; enforce capability manifests and policy at spawn.

- Legacy bridge (status)
- The legacy service bridge and its launch flags have been removed. Debug UI assets now live under `apps/arw-server/assets` and render when `ARW_DEBUG=1`.
- `/events` is the canonical SSE endpoint; the legacy `/admin/events` alias has been removed.
- Legacy feature migration (unified target — completed)
  - ✅ Core services: Model Steward, Tool Forge, Feedback Loop, Experiment Deck, Memory Lanes, Project Hub primitives, Project Map read models, Snappy Governor, and Event Spine patch streaming now run on the unified server.
  - ✅ UI/experience: Chat Workbench, Screenshot Pipeline, Self Card, and forecasts operate through the SPA/right-sidecar flow with live SSE data.
  - ✅ Policy & safety: Guardrail Gateway and Asimov Capsule Guard (alpha) enforcement live on `arw-server`, and launcher/SPA surfaces no longer depend on `/admin/*` fallbacks.

### Immediate Reintegration Backlog (Release Gate)

> Release gate: every open issue with `release-blocker:restructure` is linked to the “Immediate Reintegration” view in the ARW Restructure project. Packaging and tag automation must fail if any remain open.
> Automation: `.github/workflows/release-packages.yml` runs a preflight job that aborts releases when these blockers are present; keep labels in sync with the board.

- **Models & egress**
  - ✅ Resume support (Range/If-Range, `.part` reuse) now ships in `arw-server`.
  - [x] Re-introduce the optional HEAD quota preflight and single-flight hash guard so duplicate downloads coalesce before the GET (`release-blocker:models-egress`).
  - [x] Surface idle-timeout/retry tuneables (`ARW_DL_IDLE_TIMEOUT_SECS`, backoff policy) and expose resumable error hints in `/admin/ui/models` (`release-blocker:models-egress`).
    - Runtime metrics now publish `runtime { idle_timeout_secs, send_retries, stream_retries, retry_backoff_ms, preflight_enabled }`, and the admin UI surfaces guidance when downloads stall or resume fails.
- **Events & telemetry**
  - ✅ `egress.preview`/`egress.ledger.appended` emit from unified downloads with `corr_id` and posture metadata.
- ✅ Expanded `/state/models_metrics` with resume counters, hash-group inflight listings, and EWMA telemetry (typed schema + tests).
- **Docs & rollout**
  - ✅ Guide updates cover resume semantics and correlation IDs.
- ✅ README and release notes highlight the bridge retirement and unified entry points; subsequent releases reference the unified server exclusively.
- **UI follow-ups**
  - ✅ Launcher/admin models UI shows status badges for `resumed`/`degraded`/`cancel-requested`, and adds inline ledger previews with copy helpers for correlation IDs.
- ✅ Debug Models/Agents/Projects pages (and launcher mirrors) now call `/admin/*` endpoints with admin headers. Legacy `/models/*` shims are no longer required.

All restructure-labelled blockers are currently closed; the gate remains in CI but should pass cleanly unless new blockers are tagged.

### Legacy Feature Migration Track (runs parallel to phases 2–8)

#### Snapshot
- **Phase D — Operator experience:** Completed — chat workbench routes, screenshot pipeline wiring, and the SPA/right-sidecar migration shipped together; launcher surfaces now rely exclusively on `/state/*` endpoints.
- Recent: the admin chat endpoints now run through the shared `chat.respond` tool, exercising llama/OpenAI/LiteLLM backends via the unified action pipeline while preserving synthetic fallback for air-gapped installs.
- Debug UI exposes the new temperature and vote-k controls (wired straight into `chat.respond`), keeping observability inline with Phase D expectations.
- **Phase E — Safety:** Completed — Guardrail Gateway enforcement and Asimov Capsule Guard (alpha) coverage now run on `arw-server`, and `/admin/*` fallbacks have been deleted after capsule telemetry held steady.
- **Legacy shutdown instrumentation:** Completed — dashboards now track `arw_legacy_capsule_headers_total`; keep it pinned until the counter stays at zero for a sustained window.

| Phase | Focus | Features/Deliverables | Dependencies | Status |
| --- | --- | --- | --- | --- |
| A | Core services | Model Steward (models download/CAS GC ✅), Tool Forge (tool runs/cache metrics ✅), Snappy Governor (route stats view), Event Spine patch streaming | Triad kernel, metrics plumbing | Completed — backend landed in unified server (✅) |
| B | Memory + projects | Memory Lanes (lane CRUD/save/load), Project Hub primitives (notes/files/patch), Project Map read models (observations/beliefs/intents) | Phase A storage, policy leases | Completed — APIs live; debug UI now targets the unified routes (✅) |
| C | Feedback & experiments | Feedback Loop surfaces, Experiment Deck APIs, Self Card snapshots | Phase B data wiring | Completed — services emitting; UI polish tracked with Phase D (✅) |
| D | Operator experience | Chat Workbench, Screenshot Pipeline, launcher shift to SPA/right-sidecar, retire `/admin/*` debug windows | Phase A endpoints, UI unification groundwork | Completed — debug surfaces ship from `arw-server`; remaining SPA polish tracked via standard backlog (✅) |
| E | Safety | Guardrail Gateway on `arw-server`, Asimov Capsule Guard enforcement, final removal of legacy `/admin/*` shims | Policy & egress firewall phase | Completed — guardrail gateway + capsule guard alpha landed; propagation hooks now live in the security backlog (✅) |

Notes
- Phases can overlap if dependencies are satisfied; track owners and dates in the backlog so the matrix stays current.
- Update this section with status markers as phases complete (e.g., `A ✅`).
- Scripts: `scripts/start.{sh,ps1}` launch the unified server (launcher included). Use `ARW_NO_LAUNCHER=1` / `--service-only` for headless mode.
- Debug helpers `scripts/debug.{sh,ps1}` default to the unified stack and open `/admin/debug` when `ARW_DEBUG=1`.
  - Containers target `arw-server`; the legacy image is no longer published.
- Deprecation signals: `/admin/state/*` read-model bridges have been removed in favour of the canonical `/state/*` endpoints (review queues now live under `/admin/memory/*` and `/admin/world_diffs`). `/admin/projects/*` helpers have been removed in favour of `/state/projects*` and `/projects*`; monitor automation for any lingering calls.

### Regression priority (Unified server)

| Priority | Feature | Problem | Suggested Work |
| --- | --- | --- | --- |
| High | Admin debug surfaces (Models/Agents/Projects) | ✅ Static pages now target `/admin/*` and include admin-token handling; keep an eye out for remaining legacy calls in automation. | Monitor for CLI/scripts still pointing at legacy paths; add regression tests if gaps appear. |
| High | gRPC surface | ✅ Feature-gated gRPC listener (health/actions/events) now ships with `arw-server` (see `docs/guide/grpc.md`). | Track follow-up RPC coverage (leases, tools, rich event replay) once consumers signal demand. |
| High | Guardrail Gateway & capsules | Proxy preview exists; DNS guard and capsule adoption landed, plus live lease snapshots & events. Background refresh loop now renews capsules without relying on request traffic (`apps/arw-server/src/capsule_guard.rs`). | Surface posture presets in UI/CLI and extend network-scope enforcement across connectors/orchestrator consumers. |
| High | Model Steward resilience | ✅ HEAD preflight + single-flight hash guard restored; `/state/models_metrics` now surfaces inflight hashes and concurrency snapshots. | Track admin UI updates to render new metrics and expose preflight status; monitor ledger previews for multi-tenant downloads. |
| Medium | Chat Workbench | ✅ `/admin/chat*` endpoints restored with planner, temperature, and vote-k controls; ensure docs/tests cover multi-backend flows. | Add launcher/docs walkthroughs, exercise synthetic + remote backends in CI, and keep OpenAPI examples fresh. |
| Medium | Human-in-the-loop staging | Kernel staging queue, approvals, and debug/launcher flows are live (`docs/guide/human_in_loop.md`); next focus is richer evidence previews and posture-driven policies. | Ship sidecar evidence panes, escalation rules, and SLA alerts. |
| Medium | Research Watcher | Poller, read-model, and approve/archive APIs are live (`docs/guide/research_watcher.md`); feeds can now seed Suggested units. | Layer prioritisation heuristics, richer payload rendering, and cross-install sharing. |
| Medium | Training Park metrics | Launcher pane remains a stub, but `/state/training/telemetry` + `training_metrics`/`context_metrics` read-models are live (`docs/guide/training_park.md`). | Extend telemetry coverage, add adjustment actions, and bind UI charts. |
| Medium | Interactive snappy bench | ✅ `snappy-bench` CLI hits `/actions` + `/events`, enforces budgets, and publishes quick-start docs. | ✅ CI runs `scripts/ci_snappy_bench.py` (queue budget 500 ms); capture long-term baselines per performance preset. |

### Legacy Retirement Checklist
- Instrumentation is in place: legacy capsule headers increment `arw_legacy_capsule_headers_total`; the `/debug` alias is removed, so watch access logs or 404 gauges (or run `scripts/check_legacy_surface.sh` / `just legacy-check` for a quick static + optional HTTP probe when a server is running) for any lingering requests.
- Packaging no longer bundles the legacy bridge binaries; launcher distributions ship `arw-server` exclusively.
- Capsule leases refresh before every action submission, egress policy resolution, and tool invocation; passive renewals emit `policy.capsule.expired` without requiring HTTP traffic through the middleware.
- Guardrail defaults now assume DNS guard + loopback proxy unless explicitly disabled (`ARW_DNS_GUARD_ENABLE=0`, `ARW_EGRESS_PROXY_ENABLE=0`), keeping new nodes locked down by default.
- Start scripts (`scripts/start.{sh,ps1}`) and interactive helpers inherit those hardened defaults; verify staging starts without overriding them and document any environments that must opt out.
- Gating key docs are enforced in CI: `render_markdown`/`render_json` fixtures must match `docs/GATING_KEYS.{md,json}`, preventing drift between code and references.
- Publish this checklist with release notes when announcing final legacy shutdown so automation owners can verify metrics trends (`arw_legacy_capsule_headers_total → 0`) before the cutover window.

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
9) Decommission legacy service bridge ✅

## What’s Implemented (Quick Index)
- Kernel + CAS: `crates/arw-kernel/src/lib.rs`
- New server (triad slice): `apps/arw-server/src/main.rs`
- SSE replay bridge: `apps/arw-server/src/api/events.rs`
- Actions API: `apps/arw-server/src/api/actions.rs`
- Contribution ledger view: `GET /state/contributions` (new server)
 - Local worker (demo): dequeues queued → running → completed; appends `task.complete` to contributions.
- Orchestrator core: `crates/arw-core/src/orchestrator/` (types, queue traits, local/NATS backends)

## API Snapshot (new server)
- `POST /actions` → `{ id }` (202 Accepted)
- `GET /actions/:id` → `{ id, kind, state, input, created, updated }`
- `POST /actions/:id/state` → `{ ok: true }` (and event `actions.*`)
- `GET /events` → SSE (live bus; DB dual‑write)
- `GET /state/episodes` → `{ items: [{ id, events, items, count, errors, start, end, last, duration_ms, first_kind, last_kind, projects?, actors?, kinds? }] }` (filters via `?limit=`, `?project=` for slug/prefix, `?errors_only=`, `?kind_prefix=`, `?since=`)
- `GET /state/episode/{id}/snapshot` → `{ version, episode: { ... } }` (optional `?limit=` to cap event count)
- `GET /state/route_stats` → `{ bus: {…}, events: { start, total, kinds }, routes: { by_path: { "/path": { hits, errors, ewma_ms, p95_ms, max_ms } } } }`
- `GET /state/actions` → `{ items: [{ id, kind, state, created, updated }] }`
- `GET /state/contributions` → `{ items: [...] }`
- `GET /state/egress` → `{ count, items, settings }`
- `GET /state/egress/settings` → effective egress posture and toggles
- `POST /egress/settings` → admin‑gated runtime update of egress toggles
- `POST /egress/preview` → `{ allow, reason?, host, port, protocol }` (applies allowlist, IP‑literal guard, and policy/lease rules; logs when ledger enabled)
- `GET /state/models` → `{ items: [...] }` (reads `state/models.json` or returns defaults)
- `GET /state/memory` → SSE stream of `memory.snapshot` + `memory.patch` events (JSON Patch payloads + live snapshot)
- `GET /state/self` → `{ agents: [ ... ] }` (lists `state/self/*.json`)
- `GET /state/self/:agent` → the JSON content of `state/self/:agent.json`
 - `GET /about` → service metadata and discovery index; includes `endpoints[]` and `endpoints_meta[]` with `{ method, path, stability }` derived from in‑code path constants and route builders (avoids drift)
- `POST /leases` → `{ id, ttl_until, created, scope?, budget? }` (creates lease, emits `leases.created`, refreshes `policy_leases` read model)
- `GET /state/leases` → `{ generated, count, items: [...] }`
- `GET /state/policy` → `{ allow_all, lease_rules[] }`
- `POST /context/assemble` → assemble working set (hybrid memory retrieval; returns beliefs, seeds, and diagnostics; accepts optional `corr_id` to stitch events and publishes `working_set.*` on the main bus)
- `POST /context/rehydrate` → return full content head for a pointer (`file` head bytes or full `memory` record), gated by leases when policy requires

Events
- Egress ledger appends publish `egress.ledger.appended` with `{ id?, decision, reason?, dest_host?, dest_port?, protocol?, bytes_in?, bytes_out?, corr_id?, proj?, posture, meta }`. Meta captures policy posture, candidate capabilities, lease hits, and dns_guard flags for downstream tooling.
- Lease lifecycle publishes `leases.created` and updates the `policy_leases` read model/SSE patches.
- Policy decisions emit `policy.decision` when an action is denied or lease‑gated (payload includes `action`, `allow`, `require_capability?`, and `explain`).
- SSE filters and resume: `/events?prefix=...` filters server‑side; `/events?replay=N` replays last N; `/events?after=<row_id>` replays after a journal id; honor `Last-Event-ID` as `after` when present.

Runtime
- Async Tool Host: `ToolHost` is async. `arw-wasi::LocalHost` implements `http.fetch`, `fs.patch`, and `app.vscode.open`. `http.fetch` enforces `ARW_NET_ALLOWLIST`, `ARW_HTTP_TIMEOUT_SECS`, and `ARW_HTTP_BODY_HEAD_KB`; appends to the egress ledger and emits events.
- `fs.patch` writes atomically under `ARW_STATE_DIR/projects` (or inside that root when given a relative path). Optional `pre_sha256` precondition prevents lost updates. Emits `projects.file.written`.

## Running Locally (new server)
```bash
export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"
# Start the server with a matching token (omit --debug in hardened setups)
cargo run -p arw-server
# Health
curl -s localhost:8091/healthz
# About / version / docs
curl -s localhost:8091/about | jq
# Submit an action
curl -s -X POST localhost:8091/actions -H 'content-type: application/json' \
  -d '{"kind":"demo.echo","input":{"msg":"hi"},"idem_key":"demo-1"}'
# Stream events
curl -N -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/events
# Views (admin-gated)
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/state/episodes | jq
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/state/contributions | jq
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/state/logic_units | jq
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/state/orchestrator/jobs | jq
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" 'localhost:8091/state/memory/recent?limit=50' | jq
```

Generate the token with an equivalent tool if `openssl` is unavailable.

Notes
- Admin endpoints require either `ARW_DEBUG=1` (development-only) or a matching `Authorization: Bearer` token.
- The demo server binds to `127.0.0.1:8091` by default. Override with `ARW_BIND` and `ARW_PORT`.

## How To Try (End-to-End)

Environment and server
- Run: `ARW_STATE_DIR=state cargo run -p arw-server`

Policy (ABAC facade)
- Create `policy.json`:
  - `{ "allow_all": false, "lease_rules": [ { "kind_prefix": "net.http.", "capability": "net:http" }, { "kind_prefix": "context.rehydrate.memory", "capability": "context:rehydrate:memory" }, { "kind_prefix": "context.rehydrate", "capability": "context:rehydrate:file" } ] }`
- Export: `export ARW_POLICY_FILE=policy.json`
- Check: `curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/state/policy | jq`
- Simulate: `curl -s -X POST localhost:8091/policy/simulate -H 'content-type: application/json' -d '{"kind":"net.http.get"}' | jq`

Leases
- Create HTTP lease: `curl -s -X POST localhost:8091/leases -H 'content-type: application/json' -d '{"capability":"net:http","ttl_secs":600}' | jq`
- Create context leases: `curl -s -X POST localhost:8091/leases -H 'content-type: application/json' -d '{"capability":"context:rehydrate:file","ttl_secs":600}' | jq`
- Also grant memory rehydrate: `curl -s -X POST localhost:8091/leases -H 'content-type: application/json' -d '{"capability":"context:rehydrate:memory","ttl_secs":600}' | jq`
- List: `curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/state/leases | jq`

Context
- Assemble: `curl -s -X POST localhost:8091/context/assemble -H 'content-type: application/json' -d '{"q":"term","lanes":["semantic","procedural"],"limit":18,"include_sources":true,"corr_id":"demo-ctx-1"}' | jq`
- Rehydrate file (lease‑gated): `curl -s -X POST localhost:8091/context/rehydrate -H 'content-type: application/json' -d '{"ptr":{"kind":"file","path":"state/projects/demo/notes.md"}}' | jq`
- Rehydrate memory (lease‑gated): `curl -s -X POST localhost:8091/context/rehydrate -H 'content-type: application/json' -d '{"ptr":{"kind":"memory","id":"<memory-id>"}}' | jq`  _(use `/state/memory/recent` to find ids)_

Actions and Events
- Submit: `curl -s -X POST localhost:8091/actions -H 'content-type: application/json' -d '{"kind":"net.http.get","input":{"url":"https://example.com"}}' | jq`
- Watch: `curl -N -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/events?replay=20`

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
- Egress: `curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/state/egress | jq`
- Actions: `curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/state/actions | jq`
- Memory (stream): `curl -N -H "Authorization: Bearer $ARW_ADMIN_TOKEN" localhost:8091/state/memory`

## Contributor Checklist (Restructure)
- When adding/changing triad endpoints, kernel schemas, or runtime/policy:
  1. Update this file.
  2. Add or adjust `/state/*` views where applicable.
  3. Document flags/envs in [Configuration](CONFIGURATION.md).
  4. If it touches federation/economics, append to Contribution & Splits section.

## Post-Restructure Follow-ups
- Cedar ABAC entity modeling now feeds explainers; future refinements (relationship graph, decision caching) live in Backlog → Security Hardening & Observability.
- WASI runtime host is in place with the local tool runner; new plugins and guard hooks are tracked in Backlog (runtime/tooling stream).
- Egress proxy, DNS guard, and ledger automation stay under Security Hardening; extend posture presets rather than reopening restructure tasks.
- Memory hygiene (quarantine, world diff review) and expanded context workflows continue in Never-Out-Of-Context backlog items.
- Any new legacy migrations should land as regular backlog issues; this handbook remains as reference for the completed restructure.

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
