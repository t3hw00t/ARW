---
title: Backlog
---

# Backlog

Updated: 2025-09-29
Type: Reference

This backlog captures concrete work items and near-term priorities. The Roadmap focuses on higher‑level themes and time horizons; see About → Roadmap for strategic context.

Status: unless noted, items are todo. Items that have task IDs link to the tracker under Developer → Tasks.

## Scope Badges

We mirror the roadmap's scope badges so every backlog line shows which slice of Complexity Collapse it advances:

- `[Kernel]` — Hardens the runtime, policy, and journal so the “Collapse the Kernel” thrust stays minimal, dependable, and auditable.
- `[Pack: Collaboration]` — Optional collaboration, UI, and workflow packs that give calm surfaces and governance without bloating the kernel.
- `[Pack: Research]` — Optional research, experimentation, and memory packs that extend retrieval, clustering, and replay while staying pluggable.
- `[Pack: Federation]` — Optional federation packs that let multiple installs cooperate under shared policy, budgets, and accountability.
- `[Future]` — Bets incubating beyond the active quarter; they stay visible but outside the current Complexity Collapse execution window.

Badges can be combined (for example, `[Pack: Collaboration][Future]`) to show both the optional pack and that the work sits beyond the active delivery window.

## Now (Weeks)

Complexity Collapse (Cross-cutting)
- [Kernel] One service API surface (`/state`, `/events`, `/actions`) with no side channels — stabilize instrumentation and error budgets as new packs land.
- [Kernel] Single SQLite journal with content-addressed blobs; derive read-models and caches — optimize compaction + vacuum cadence and document HA restore drills.
- [Kernel] Job model & scheduler as the only execution path; unify local and remote runners — converge legacy code paths and expand coverage for remote runner fallbacks.
- [Kernel] Patch engine for all writes with diff preview and rollback — add end-to-end tests for Project/Policy flows and CI drift alerts.
- [Kernel] Documented event taxonomy; views/read-models subscribe to the event stream — keep taxonomy registry in sync with Event Spine patches and launcher consumers.
- [Kernel] Flows as DAG data executed by a single flow-runner; tools are schema-defined nodes — graduate existing flows to the unified runner and remove bespoke executors.
- [Kernel] Unified retrieval pipeline and memory abstraction (vector/graph/kv/doc) with shared CRUD/stats and index hygiene — wire production hygiene dashboards and janitor tooling.
- [Kernel] Capability/lease system with node-local egress proxy; remove per-tool allowlists — align policy manifests with lease scopes and document operator responses.
- [Kernel] UI: shared right-sidecar, schema-generated forms, and global command palette — sustain accessibility audits and ensure new packs ship with parity.
- **Recently shipped:** Legacy feature migration (Phases A–E); Snappy Governor verification; Event Spine patch streaming; Phase A handoff (see `docs/RESTRUCTURE.md` and tasks `t-phase-a-01..03`).

Managed Runtime Supervisor (Priority One)
- [Kernel] Runtime Matrix Phase 1: land health reasons, restart budgets, and accessible status strings; add CPU/GPU smoke tests exercising llama.cpp integration — in_progress (health strings + restart budgets landed; stubbed llama smoke via `just runtime-smoke`; GPU-backed runs still outstanding).
- [Kernel] Supervisor Core Phase 2: finalize `RuntimeRegistry` adapter trait, lease-gated start/stop APIs, structured logs, and policy simulator coverage — plan.
- [Pack: Collaboration][Kernel] Launcher runtime panels: expose profiles, consent cues, and start/stop controls with keyboard parity; publish operator runbook excerpt in Launcher help cards — plan.
- [Kernel] Supply-chain readiness for bundled runtimes: sign binary manifests, document update cadence, and ship rollback checklist — plan.

Never‑Out‑Of‑Context (High Priority)
- [Pack: Research] [t-250912143009-0003] Retrieval: add MMR‑style selector across vector/graph mounts and world beliefs — todo
- [Pack: Research] [t-250912143013-0004] Compression cascade: summarize episodes (extract→abstract→outline) into mounts with provenance — todo
- [Pack: Research] [t-250912143017-0005] Failure detectors: emit `context.recall.risk` and `context.coverage` with meters in UI — in_progress (recall risk events + telemetry landed; UI meters pending)
- [Pack: Research] [t-250912143025-0007] Logic Unit: ship config‑only Never‑Out‑Of‑Context defaults (budgets, diversity, rehydrate rules) — todo
- [Pack: Research] [t-250912143029-0008] UI: Project Hub panel “What’s in context now” with artifact pointers and rehydrate actions — todo
- [Pack: Research] [t-250912143033-0009] Training Park: dials for diversity/recency/compression; recall‑risk and coverage meters — todo
- [Pack: Research] [t-250918120201-tp01] Training telemetry read-models in `arw-server` (context/memory/tool success stats) powering Training Park — doing (baseline snapshot live; expanding coverage)
- [Pack: Research] [t-250918120205-tp02] Launcher Training Park window: replace stub UI with live metrics + control bindings — plan
- **Recently shipped:** Context Working Set doc; Context API budgets + stable IDs; Memory hygiene janitor; Context telemetry guardrails.

UI Coherence
- [Pack: Collaboration] Universal right‑sidecar across Hub/Chat/Training; subscribe once to `/events` — done (initial lanes)
- [Pack: Collaboration] Command Palette: global search + actions; attach agent to project; grant scoped permissions with TTL — done (initial)
- [Pack: Collaboration] Compare: Hub Text/JSON (Only changes/Wrap/Copy), Image slider, CSV/Table key‑diff — done
- [Pack: Collaboration] Compare: Chat A/B pin‑to‑compare and diff — done
- [Pack: Collaboration] Events window: presets (state/models/tools/egress/feedback), include/exclude body filters, pretty/wrap/pause — done
- [Pack: Collaboration] Events window: RPU preset (rpu.*) — done
- [Pack: Collaboration] Logs window: route filter and focus tables mode — done
- [Pack: Collaboration] Screenshots: precise screen/window/region capture (with preview), sidecar Activity thumbnails — done
- [Pack: Collaboration] OCR: default‑on build with Tesseract; Auto OCR toggle in Chat; palette toggle — done
- [Pack: Collaboration] Gallery: modal with Open/Copy/Copy MD/Save to project/Annotate — done
- [Pack: Collaboration] Annotation overlay: draw rectangles; blur+border burn; sidecar JSON — done
- [Pack: Collaboration] Save to project: server import endpoint; path prompt; toast feedback — done
- [Pack: Collaboration] Project Hub (Files): breadcrumbs + Back; filter; inline expandable tree with persisted expansions; drag‑and‑drop upload; per‑project editor overrides; Open in Editor flow — done
- [Pack: Collaboration] Project Hub (Files): notes autosave with inline status; conflict‑aware merge UI with diff + scroll‑sync — done
- [Pack: Collaboration] Project Hub (Files): expand‑on‑search (auto‑expand ancestors of matches) and match highlighting — done
- [Pack: Collaboration] Project Hub (Runs): Pin‑to‑compare available; filters are non‑persistent (view‑only) — done
- [Pack: Collaboration] Accessibility: tree roles/aria‑level/expanded; regions labeled; command palette and gallery as dialogs; focus ring on rows; Compare tabs with role=tablist and roving tabindex — done
- [Pack: Collaboration] Routes: canonicalize admin UI paths (`/admin/debug`, `/admin/ui/*`); remove the legacy `/debug` alias; update launcher open path — done
- [Pack: Collaboration] SSE store: add connection status + resilient auto-reconnect with modest backoff; reuse filters and replay across reconnect — done
- [Pack: Collaboration] Connections window: allow per-connection admin token; open Events/Logs/Models windows pointed at that base — done (launcher now stores per-connection tokens, normalises bases, surfaces status, and remote windows honour the saved base)
- [Pack: Collaboration] Per‑project templates: save/apply lanes/grid/focus in Hub — done
- [Pack: Collaboration] Route SLO selector UI: adjustable p95 threshold in Logs/Events — done
- [Pack: Collaboration] Export CSV: route/kind tables — done; table diff export — done (two‑row or wide)
- [Pack: Collaboration] Next: labels/arrows in annotator; redaction presets (regex+OCR); append Markdown to NOTES.md; Pin‑to‑compare from Runs; retention/tagging for gallery; guided countdown for capture.
 - [Pack: Collaboration] Next: keyboard shortcuts (global) cheatsheet and discoverability; ARIA polish for Agents/Runs actions; skip‑links across pages; unit tests for /projects/file content_b64 path; virtualize large trees.
- [Pack: Collaboration] [t-250918120301-hitl01] Human-in-the-loop staging queue in `arw-server` with `/state/staging/actions` read-model and leases — done (shipped kernel + API; follow-up UX tracked separately)
- [Pack: Collaboration] [t-250918120305-hitl02] Sidecar approvals UI: replace placeholder copy with live staging actions + evidence preview — done (sidecar lane shows staging queue, evidence viewer, approve/deny buttons, persisted filters/sort, stale-mode triage chips with shortcuts, copyable summaries)
- [Pack: Collaboration] Feedback loop readiness: validate Heuristic Feedback Engine shadow runs, log deltas, and document sidecar approvals before enabling auto-apply — todo
- [Pack: Collaboration] Project Hub SSE bridge: consume `state.read.model.patch` (Event Spine) for notes/files/live context in the SPA swap — doing (metadata feed wired)

Experience Outcomes
- [Pack: Collaboration] Trusted Onboarding Journey kit: scripted first-run narration, beta walk-through deck, Launcher help-card refresh — plan (ship alongside Runtime Supervisor Phase 2 enabling)
- [Pack: Collaboration] Consent UX validation sprint: moderated sessions with partner teams validating audio/vision consent dialogs and accessibility cues; publish findings brief — todo
- [Kernel] Complexity Collapse mission brief cadence: monthly stakeholder digest (wins, risks, upcoming user moments) archived into `docs/release_notes.md` — recurring (kick off with Runtime Supervisor beta)

Kernel & Triad (NOW)
- [Kernel] [t-250915090001-kern01] Add `arw-kernel` crate with SQLite/WAL schema (events, artifacts, actions) and CAS helpers — done
- [Kernel] [t-250915090010-kern02] Dual-write bus events to kernel and expose `/triad/events?replay=N` — done
- [Kernel] [t-250915090020-kern03] Add `/actions` endpoint backed by kernel with idempotency and policy stub — done (triad queue unified)
- [Kernel] [t-250915090030-kern04] Add `/state/*` views sourced from kernel (episodes, route_stats, models) — done (read-models shipped; continued polish tracked in metrics section)
- [Kernel] [t-250915090040-kern05] Migrate JSONL events journal to SQLite (remove old env) — plan

Design System & Tokens
- [Pack: Collaboration] [t-250914231200-dsg01] Single‑source tokens (CSS/JSON) under `assets/design/` — done
- [Pack: Collaboration] [t-250914231205-dsg02] Sync helper and task (`scripts/sync_tokens.sh`, `just tokens-sync`) — done
- [Pack: Collaboration] [t-250914231210-dsg03] Docs: load tokens via `extra_css`; add Design Theme page — done
- [Pack: Collaboration] [t-250914231215-dsg04] Launcher: adopt tokens across common/index/events/logs/models/connections — done (initial)
- [Pack: Collaboration] [t-250914231220-dsg05] Service Debug UI: replace inline styles with token vars — done (CSS extraction + utility classes)
- [Pack: Collaboration] [t-250914231225-dsg06] Docs overrides: dedupe variables; rely on tokens only — done
- [Pack: Collaboration] [t-250914231240-dsg09] CI: add tokens sync check step (uses `just tokens-check`) — done
- [Pack: Collaboration] [t-250914231230-dsg07] Extract `ui-kit.css` primitives (buttons/inputs/badges) for launcher pages — done
- [Pack: Collaboration] [t-250914231235-dsg08] Contrast audit (WCAG AA) sweep; adjust any low‑contrast cases — todo
- [Pack: Collaboration] [t-250914231245-dsg10] Add W3C tokens pipeline (Style Dictionary) to emit platform targets — plan
- [Pack: Collaboration] [t-250914231250-dsg11] Add prefers-contrast / forced-colors styles for key components — done (launcher + shared UI kit high-contrast styles)
 - [Pack: Collaboration] [t-250914231255-dsg12] Tailwind tokens export (JSON) for downstream configs — done

Standards & Docs
- [Kernel] [t-250914231255-std01] Add ADR framework and seed first ADRs (tokens SSoT, event naming) — plan
 - [Kernel] [t-250914231300-std02] Optional docs a11y check in CI (axe) — done

Recipes MVP
- [Pack: Collaboration] Finalize `spec/schemas/recipe_manifest.json`; add doc‑tests and example gallery entries
- [Pack: Collaboration] Gallery UI: install (copy folder), inspect manifest, launch, capture Episodes
- [Pack: Collaboration] Permission prompts (ask/allow/never) with TTL leases; audit decisions as `Policy.*` events

Logic Units (High Priority)
- [Pack: Research] Manifest and schema (`spec/schemas/logic_unit_manifest.json`); example units under `examples/logic-units/`
- [Pack: Research] Library UI: tabs (Installed/Experimental/Suggested/Archived), diff preview, apply/revert/promote
- [Pack: Research] Agent Profile slots + compatibility checks (design + stubs)
- [Pack: Research] A/B dry‑run pipeline wired to Evaluation Harness; per‑unit metrics panel
- [Pack: Research] [t-250918120101-rw01] Research Watcher ingestion service in `arw-server` (RSS/OpenReview adapters queued to kernel-backed jobs) — done (phase one JSON feeds)
- [Pack: Research] [t-250918120105-rw02] `/state/research_watcher` read-model + `state.read.model.patch` stream for Suggested tab — done (launcher + debug consuming live patches)
- [Pack: Research] [t-250918120109-rw03] Launcher Library integration: surface Suggested units with approve/archive actions wired to new endpoints — doing (polish bulk actions/tags)

Last‑mile Structures
- [Pack: Research] Config Patch Engine: dry‑run/apply/revert endpoints; schema validation; audited permission widening
- [Pack: Research] Experiment Orchestrator: start/stop/assign; emit Experiment.*; fold to `/state/experiments`
- [Pack: Research] Provenance/Snapshots: enrich `/state/episode/{id}/snapshot` with effective config (units, params, model hash, policies)
- [Pack: Research] AppSec Harness: seed tests; surface violations as `policy.decision` events; block unsafe tool I/O
- [Pack: Research] Observability (OTel): map timeline to traces (corr_id as trace); correlate metrics/logs
- [Pack: Research] Compliance Mode: workspace switch; record‑keeping + approvals; UI status widget
- [Pack: Research] Supply‑Chain Trust: signed manifests, SBOMs, sandbox defaults; align desktop capabilities with policies
- [Pack: Research] Scheduler/Governor: fair queues, preemption, backpressure, kill‑switch; policy‑aware

Security & Admin
- [Kernel] Admin auth hardening — hashed tokens + per‑token/IP sliding rate‑limit [t-250911230312-0863]
- [Kernel] Per‑route gating layers; slim global admin middleware [t-250911230252-9858]
- [Kernel] Supply‑chain: upgrade GTK/GLib stack to >=0.20 (via wry/gtk/tao/tauri) to resolve RUSTSEC-2024-0429; remove temporary ignore in `deny.toml` and audit script guard once lockfile carries `glib >= 0.20.0`.

- [Kernel] Asimov Capsule Guard (alpha)
  - [Kernel] [t-250916130001-asg01] RPU telemetry + `/state/policy/capsules` read-model surfacing adoption and TTL — done (capsule leases, expired events)
  - [Kernel] [t-250916130002-asg02] Layered gating denies/contracts with lease sweeper + emergency teardown hook — partial (lease sweeper merged; teardown UX remains)
  - [Kernel] [t-250916130003-asg03] Auto-replay verified capsules before actions/tools/egress/policy evaluation — done for actions/tools; extend to proxy + Logic Unit runners (backlog)
  - [Kernel] [t-250916130004-asg04] Admin UX + CLI for capsule presets, rotation, and audit trails — backlog (ties into Phase 3 UX controls)
Caching & Performance (High Priority)
- [Kernel] [t-250913001000-1001] Llama.cpp prompt cache: set `cache_prompt: true` in requests; doc server `--prompt-cache` for persistence — in progress
- [Kernel] [t-250913001003-1002] CAS HTTP caching: add `ETag`, `Last-Modified`, long‑lived `Cache-Control`, and 304 handling to `/admin/models/by-hash/{sha256}` — done
- [Kernel] [t-250913001006-1003] Action Cache (MVP): wrap `tools_exec::run` with deterministic key (tool id, version, canonical JSON, env signature stub) and CAS’d outputs; Moka front with TTL; `tool.cache` events — in progress
- [Kernel] [t-250913001009-1004] Singleflight: coalesce identical in-flight tool runs and expensive read-model recomputes — done (shared guard now covers tool cache + read-model snapshots)
- [Kernel] [t-250913001012-1005] Read‑models SSE deltas: stream JSON Patch with `Last-Event-ID` resume; wire Debug UI to apply patches — done (kernel SSE ids wired, debug UI applies patches, TS client exposes subscribeReadModel + stream helper, CLI tails via generator)
- [Kernel] [t-250913001015-1006] Metrics: expose cache hit/miss/age, bytes/latency saved, stampede suppression rate at `/state/*` and `/metrics` — done (tool cache telemetry surfaces + Prometheus counters)
- [Kernel] [t-250913001018-1007] Cache Policy manifest + loader: define YAML format, map to env knobs, and plan migration to config‑first overrides — done (loader applies manifest on startup with env override tracking)
- [Kernel] [t-250914210100-http01] HTTP helpers module (`api::http_utils`) for ETag/Last-Modified/Range parsing; adopt in models blob GET/HEAD — done
- [Kernel] [t-250914210104-http02] Adopt `api::http_utils` helpers across any future digest/static file endpoints (keep semantics consistent) — done (models blob CAS route)
- [Kernel] [t-250914210107-http03] Docs: consolidate HTTP caching semantics into a short reusable snippet and cross‑link from API/Guide — done

Egress Firewall & Posture (Plan)
- [Kernel] Policy: add network scopes (domain/IP/CIDR, port, protocol) and TTL leases; surface in UI.
- [Kernel] Gateway: per‑node loopback proxy (HTTP(S)/WS CONNECT; optional SOCKS5) with allow/deny by SNI/Host and port; deny IP‑literals by default; no TLS MITM.
- [Kernel] DNS Guard: force resolver, block UDP/53/DoH/DoT from tools; short TTLs; log lookups.
- [Kernel] Routing: containers via proxy env + blocked direct egress; host processes with OS firewall rules (allow 127.0.0.1:proxy only for agent PIDs).
- [Kernel] Ledger: append‑only egress ledger with episode/project/node attribution and bytes/$ estimates; pre‑offload preview dialog in UI.
- [Kernel] Posture: Off/Public/Allowlist/Custom per project; default to Public.
- [Kernel] Filesystem & Sensors: sandbox write scopes (project://) and leased mic/cam sidecar access with TTL + audits.
- [Kernel] Cluster: replicate gateway+DNS guard per Worker; propagate policies from Home over mTLS; Workers cannot widen scope.

Lightweight Mitigations (Plan)
- [Kernel] Memory quarantine: add review queue and `memory.quarantined`/`memory.admitted` events; admit only with provenance + evidence score.
- [Kernel] Project isolation: enforce per‑project namespaces for caches/embeddings/indexes; “export views” only; imports read‑only and revocable.
- [Kernel] Belief‑graph ingest: queue world diffs; surface conflicts; require accept/apply with audit events.
- [Kernel] Cluster manifest pinning: define signed manifest schema; publish/verify; scheduler filters to trusted manifests.
- [Kernel] Secrets hygiene: vault‑only; redaction pass on snapshots/egress previews; secret‑scanner job for artifacts.
- [Kernel] Hardened headless browsing: disable service workers/HTTP3; same‑origin fetches; DOM‑to‑text extractor; route via proxy.
- [Kernel] Safe archive handling: temp jail extraction with path canonicalization; size/time caps; nested depth limit.
- [Kernel] DNS guard + anomaly: rate‑limit lookups; alert on high‑entropy domain bursts.
- [Kernel] Accelerator hygiene: zero VRAM/buffers between jobs; disable persistence mode where possible; prefer per‑job processes.
- [Kernel] Co‑drive role separation: roles view/suggest/drive; “drive” cannot widen scope or approve leases; tag remote actions.
- [Kernel] Event integrity: mTLS; nonces; monotonic sequence numbers; idempotent actions; reject out‑of‑order/duplicates.
- [Kernel] Context rehydration guard: redaction/classification before reuse in prompts; badge “potentially exportable”; require egress lease if offloaded.
- [Kernel] Operational guardrails: per‑project security posture (Relaxed/Standard/Strict); egress ledger retention + daily review UI; one‑click revoke.
- [Kernel] Hygiene cadence: quarterly key rotation & re‑sign; monthly dependency sweep with golden tests & snapshot diffs.
- [Kernel] Seeded red‑team tests in CI: prompt‑injection, zip‑slip, SSRF, secrets‑in‑logs detector.

Remote Access & TLS
- [Kernel] Dev TLS profiles (mkcert + self‑signed) for localhost
- [Kernel] Caddy production profile with Let's Encrypt (HTTP‑01/DNS‑01) for public domains
- [Kernel] Reverse‑proxy templates (nginx/caddy) with quick run/stop helpers
- [Kernel] Secrets handling: persist admin tokens only to local env files; avoid committing to configs
- [Kernel] Setup wizards to pick domain/email, validate DNS, and dry‑run cert issuance

Observability & Eventing
- [Kernel] Event journal: reader endpoint (tail N) and topic‑filtered consumers across workers/connectors — done (GET /admin/events/journal with prefix filters; connectors consume via subscribe_filtered; `arw-cli events journal --follow` tails from the CLI)
- [Kernel] Metrics registry with histograms; wire to /metrics [t-250911230320-8615]
- [Kernel] Docs: surface route metrics/events in docs and status page — done
- [Pack: Collaboration] RPU trust: watcher + endpoints + `rpu.trust.changed` event + Prometheus gauges — done
- [Kernel] Event reader QA: test `Last-Event-ID` resume, ensure Spectral/OpenAPI coverage, and capture doc updates in developer guide — done (tests/spec/docs refreshed in this change)

Compatibility & Hardware
- [Kernel] GPU probe fallback via wgpu: enumerate adapters across backends — done

State Read‑Models & Episodes
- [Kernel] Observations read‑model + GET /state/observations [t-250912001055-0044]
- [Kernel] Beliefs/Intents/Actions stores + endpoints [t-250912001100-3438]
- [Kernel] Episodes + Debug UI reactive views (truth window) [t-250912001105-7850]
- [Kernel] Debug UI: Episodes filters + details toggle [t-250912024838-4137]

Hierarchy & Governor Services
- [Kernel] Encapsulate hierarchy (hello/offer/accept/state/role_set) and governor (profile/hints) into typed services; endpoints prefer services; publish corr_id events; persist orchestration [t-250912024843-7597]

CLI & Introspection
- [Kernel] Migrate arw-cli to clap (derive, help, completions, JSON flag) [t-250911230329-4722]
- [Kernel] Auto‑generate /about endpoints from router/introspection [t-250911230306-7961] — done
  - [Kernel] /about merges public endpoints (runtime recorder) and admin endpoints (macro registry); entries are `METHOD path`; deduped and sorted.

Queues, NATS & Orchestration
- [Kernel] Orchestrator: lease handling, nack(retry_after_ms), group max in‑flight + tests [t-250911230308-0779]
- [Kernel] NATS: TLS/auth config and reconnect/backoff tuning; docs and examples [t-250911230316-4765]

Specs & Docs
- [Kernel] Generate AsyncAPI + MCP artifacts and serve under /spec/* [t-250909224102-9629]
- [Kernel] Docgen: gating keys listing + config schema and examples — done (CLI docgen + gating_config reference)
- [Kernel] AsyncAPI: include `rpu.trust.changed` channel — done
- [Kernel] Event normalization rollout
  - [Kernel] [t-250913213500-ev01] Add dual-mode warning logs when ARW_EVENTS_KIND_MODE=dual to surface remaining legacy consumers — removed (modes dropped)
  - [Kernel] [t-250913213501-ev02] Update all docs/screenshots/snippets to normalized kinds (models.download.progress, …) — in progress
  - [Kernel] Gallery guide and screenshots guide updates — done
- [Kernel] [t-250913213502-ev03] Add envelope schema `ApiEnvelope<T>` in OpenAPI and adopt in responses (opt-in) — done (global middleware + schema; toggle via `ARW_API_ENVELOPE`)
  - [Kernel] [t-250913213503-ev04] Add short descriptions to any endpoints with Spectral hints (e.g., /state/models) — done
  - [Kernel] [t-250913213504-ev05] Adjust Spectral AsyncAPI rule to accept dot.case explicitly to remove naming warnings — done (custom dot.case checker in Spectral ruleset)
  - [Kernel] [t-250913213505-ev06] Plan removal: switch all deployments to normalized kinds, then drop legacy/dual paths — done
  - [Kernel] [t-250913213506-ev07] Add deprecation note to release notes (legacy event kinds) — done

Visual Capture (Screenshots)
- [Pack: Collaboration] Screenshot tool (OS‑level): capture entire screen/display or region; save to `.arw/screenshots/` and emit `screenshots.captured` — done
- [Pack: Collaboration] Window crop: obtain Tauri window bounds and crop screenshot to window rectangle — done
- [Pack: Collaboration] OCR pass (optional): run OCR over captured region (tesseract/rapidocr) to extract text for search — in progress (feature‑flag ready; enabled by default)
- [Pack: Collaboration] Sidecar: Activity lane shows recent screenshots as thumbnails with open/copy actions — done
- [Pack: Collaboration] Annotation: overlay + burn tool with sidecar JSON; blur+border — done
- [Pack: Collaboration] Gallery: modal with actions and annotate — done
- [Pack: Collaboration] Policy: gate under `io:screenshot` and `io:ocr`; leases and audit — done
- [Pack: Collaboration] Next: redaction presets; labels/arrows; retention and search in gallery; Save to project macro to append Markdown in NOTES.md.

Strict dot.case normalization (no back-compat)
- [Kernel] [t-250914050900-ev10] Update topics SSoT to dot.case only (remove CamelCase constants) — done (topics audit confirmed all values normalized; 2025-09-28 sweep)
- [Kernel] [t-250914050902-ev11] Replace all publishers to use topics.rs constants (no hard-coded strings) — done (runtime + matrix now emit via TOPIC_* constants; this sweep)
- [Kernel] [t-250914050904-ev12] Debug UI: switch listeners to dot.case only — done
- [Kernel] [t-250914050906-ev13] Connector: publish `task.completed` and subjects `arw.events.task.completed` + node variant — done
- [Kernel] [t-250914050908-ev14] Update Feature Matrix topics to dot.case — done
- [Kernel] [t-250914050910-ev15] Docs: update Events Vocabulary/Topics/Admin Endpoints to dot.case — done
- [Kernel] [t-250914050912-ev16] CI linter: fail on any `publish("...CamelCase...")` or legacy subjects — done (CI guard + CLI/self-test helpers; this change)
- [Kernel] [t-250914050914-ev17] arw-core gating keys: add `events:task.completed` and update callers — done (gating key shipped in arw-core; docs and tests updated)
- [Kernel] [t-250914050916-ev18] Release notes: breaking change, mapping table, migration notes — done (dot.case mapping table + migration checklist added to release notes)

OpenAPI/Examples
- [Kernel] [t-250913213507-api01] Add examples for /admin/models/jobs and /admin/models/download responses — done
- [Kernel] [t-250913213508-api02] Document public /state/models envelope explicitly or add note about envelope omission in examples — done (docs/API_AND_SCHEMA.md)

Feedback Engine (Near‑Live)
- [Pack: Collaboration] Engine crate and integration: actor with O(1) stats, deltas via bus, snapshot+persistence [t-250909224102-8952]
- [Pack: Collaboration] UI: near-live feedback in /admin/debug showing deltas with rationale/confidence [t-250909224103-0211]
- [Pack: Collaboration] Policy hook: shadow → policy‑gated auto‑apply with bounds/rate‑limits [t-250909224103-5251]

Testing
- [Kernel] End‑to‑end coverage for endpoints & gating; fixtures; CI integration [t-250911230325-2116]
- [Kernel] [t-250914210200-test01] Scoped state dir for tests (`test_support::scoped_state_dir`) to isolate `ARW_STATE_DIR` — done
- [Kernel] [t-250914210204-test02] Migrate env‑derived `state_dir` lookups to a process‑lifetime cache (OnceCell) with a test‑only reset hook to avoid flakiness — done (scoped cache + guard landed)
- [Kernel] [t-250914210208-test03] Concurrency controls: add tests for `block=false` shrink path and pending_shrink reporting — done (apps/arw-server/src/models.rs)

Stabilization & Contracts
- [Kernel] [t-250914210300-api01] Models summary: switch handler to typed snapshots from `ModelsService` (avoid ad‑hoc JSON picks) — done
- [Kernel] [t-250914210302-api06] Concurrency: expose `pending_shrink` in `GET /admin/models/concurrency` and jobs snapshot — done
- [Kernel] [t-250914210303-ui01] Models UI: add Jobs panel, concurrency controls with feedback, Installed Hashes filters + reset, and persistence — done
- [Kernel] [t-250914210304-ui02] Installed Hashes: add pagination controls (offset/next/prev) — done (pagination metadata + launcher controls)
- [Kernel] [t-250914210304-api02] Egress ledger: add compact summarizer endpoint (time‑bounded; filters by decision/reason) — done
- [Kernel] [t-250914210308-api03] CAS GC: centralize manifest reference extraction (single helper used by GC and any scanners) — done (confirmed `manifest_hash_index()` powers GC and `/state/models_hashes`; no duplicate scanners remain)
- [Kernel] [t-250914210312-api04] Contract tests: promote spec/docs consistency checks to CI (status/code enums already validated in unit tests) — done (curated coverage enforcement + spec regen)
- [Kernel] [t-250914210316-api05] Sweep and remove any remaining deprecated endpoints; update docs and SDKs — done (removed /admin/introspect/stats; updated debug UI to use /state/route_stats; specs/docs refreshed)

## Next (1–2 Months)

Platform & Policy
- [Kernel] WASI plugin sandbox: capability‑based tool permissions (ties to Policy)
- [Kernel] Policy engine integration (Cedar bindings); per‑tool permission manifests
- [Pack: Collaboration] RPU: trust‑store watch + stronger verification; introspection endpoint [t-250911230333-9794]

Models & Orchestration
- [Kernel] Model orchestration adapters (llama.cpp, ONNX Runtime) with pooling and profiles
- [Kernel] Managed llama.cpp runtime supervisor: runtime registry, health probes, Launcher controls, `/state/runtimes`
- [Kernel] Runtime adapter contract + docs (llama.cpp first, ONNX Runtime pilot)
- [Pack: Collaboration] Launcher runtime panel: hardware probe, profile selector, per-project overrides
- [Pack: Research] Orchestrator runtime claims + Autonomy Lane runtime gates (`runtime.claim.*` events, fallback policy)
- [Kernel] Accelerator bundles: signed CUDA/ROCm/Metal/DirectML/CoreML/Vulkan builds + capability detection wizard
- [Pack: Collaboration] Voice & Vision adapters (managed Whisper.cpp, llava.cpp) with Launcher consent flows
- [Pack: Collaboration] Pointer & keyboard automation tools with high-trust leases, replay logging, and runtime supervisor integration
- [Kernel] Multi-modal runtime schema updates (`/state/runtimes` modalities, policy gates); see docs/architecture/multimodal_runtime_plan.md
- [Pack: Research] Capsules: record inputs/outputs/events/hints; export/import; deterministic replay

Specs & Interop
- [Kernel] AsyncAPI + MCP artifacts in CI; promote developer experience around /spec/*

Docs & Distribution
- [Pack: Collaboration] Showcase readiness: polish docs, packaging, and installer paths

## Notes
- [Kernel] For live status of all tracked tasks, see Developer → Tasks, which renders from `.arw/tasks.json`.
- [Kernel] Recently shipped work is summarized under About → Roadmap → Recently Shipped.
