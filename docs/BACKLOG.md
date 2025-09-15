---
title: Backlog
---

# Backlog

Updated: 2025-09-15
Type: Reference

This backlog captures concrete work items and near-term priorities. The Roadmap focuses on higher‑level themes and time horizons; see About → Roadmap for strategic context.

Status: unless noted, items are todo. Items that have task IDs link to the tracker under Developer → Tasks.

## Now (Weeks)

Complexity Collapse (Cross-cutting)
- One service API surface (`/state`, `/events`, `/actions`) with no side channels
- Single SQLite journal with content-addressed blobs; derive read-models and caches
- Job model & scheduler as the only execution path; unify local and remote runners
- Patch engine for all writes with diff preview and rollback
- Documented event taxonomy; views/read-models subscribe to the event stream
- Flows as DAG data executed by a single flow-runner; tools are schema-defined nodes
- Unified retrieval pipeline and memory abstraction (vector/graph/kv/doc) with shared CRUD/stats and index hygiene
- Capability/lease system with node-local egress proxy; remove per-tool allowlists
- UI: shared right-sidecar, schema-generated forms, and global command palette

Never‑Out‑Of‑Context (High Priority)
- [t-250912143001-0001] Context Working Set doc + mkdocs nav — done (this change)
- [t-250912143005-0002] Context API: allow slot budgets and return stable pointers (IDs) for all included items — todo
- [t-250912143009-0003] Retrieval: add MMR‑style selector across vector/graph mounts and world beliefs — todo
- [t-250912143013-0004] Compression cascade: summarize episodes (extract→abstract→outline) into mounts with provenance — todo
- [t-250912143017-0005] Failure detectors: emit `context.recall_risk` and `context.coverage` with meters in UI — todo
- [t-250912143021-0006] Memory hygiene: per‑lane caps + TTL + janitor job with rollups and evictions — todo
- [t-250912143025-0007] Logic Unit: ship config‑only Never‑Out‑Of‑Context defaults (budgets, diversity, rehydrate rules) — todo
- [t-250912143029-0008] UI: Project Hub panel “What’s in context now” with artifact pointers and rehydrate actions — todo
- [t-250912143033-0009] Training Park: dials for diversity/recency/compression; recall‑risk and coverage meters — todo

UI Coherence
- Universal right‑sidecar across Hub/Chat/Training; subscribe once to `/events` — done (initial lanes)
- Command Palette: global search + actions; attach agent to project; grant scoped permissions with TTL — done (initial)
- Compare: Hub Text/JSON (Only changes/Wrap/Copy), Image slider, CSV/Table key‑diff — done
- Compare: Chat A/B pin‑to‑compare and diff — done
- Events window: presets (state/models/tools/egress/feedback), include/exclude body filters, pretty/wrap/pause — done
- Events window: RPU preset (rpu.*) — done
- Logs window: route filter and focus tables mode — done
- Screenshots: precise screen/window/region capture (with preview), sidecar Activity thumbnails — done
- OCR: default‑on build with Tesseract; Auto OCR toggle in Chat; palette toggle — done
- Gallery: modal with Open/Copy/Copy MD/Save to project/Annotate — done
- Annotation overlay: draw rectangles; blur+border burn; sidecar JSON — done
- Save to project: server import endpoint; path prompt; toast feedback — done
- Project Hub (Files): breadcrumbs + Back; filter; inline expandable tree with persisted expansions; drag‑and‑drop upload; per‑project editor overrides; Open in Editor flow — done
- Project Hub (Files): notes autosave with inline status; conflict‑aware merge UI with diff + scroll‑sync — done
- Project Hub (Files): expand‑on‑search (auto‑expand ancestors of matches) and match highlighting — done
- Project Hub (Runs): Pin‑to‑compare available; filters are non‑persistent (view‑only) — done
- Accessibility: tree roles/aria‑level/expanded; regions labeled; command palette and gallery as dialogs; focus ring on rows; Compare tabs with role=tablist and roving tabindex — done
- Routes: canonicalize admin UI paths (`/admin/debug`, `/admin/ui/*`); keep local dev alias `/debug`; update launcher open path — doing
- SSE store: add connection status + resilient auto-reconnect with modest backoff; reuse filters and replay across reconnect — doing
- Connections window: allow per-connection admin token; open Events/Logs/Models windows pointed at that base — todo
- Per‑project templates: save/apply lanes/grid/focus in Hub — done
- Route SLO selector UI: adjustable p95 threshold in Logs/Events — done
- Export CSV: route/kind tables — done; table diff export — done (two‑row or wide)
- Next: labels/arrows in annotator; redaction presets (regex+OCR); append Markdown to NOTES.md; Pin‑to‑compare from Runs; retention/tagging for gallery; guided countdown for capture.
 - Next: keyboard shortcuts (global) cheatsheet and discoverability; ARIA polish for Agents/Runs actions; skip‑links across pages; unit tests for /projects/file content_b64 path; virtualize large trees.

Kernel & Triad (NOW)
- [t-250915090001-kern01] Add `arw-kernel` crate with SQLite/WAL schema (events, artifacts, actions) and CAS helpers — done
- [t-250915090010-kern02] Dual-write bus events to kernel and expose `/triad/events?replay=N` — done
- [t-250915090020-kern03] Add `/actions` endpoint backed by kernel with idempotency and policy stub — todo
- [t-250915090030-kern04] Add `/state/*` views sourced from kernel (episodes, route_stats, models) — todo
- [t-250915090040-kern05] Migrate JSONL events journal to SQLite (remove old env) — plan

Design System & Tokens
- [t-250914231200-dsg01] Single‑source tokens (CSS/JSON) under `assets/design/` — done
- [t-250914231205-dsg02] Sync helper and task (`scripts/sync_tokens.sh`, `just tokens-sync`) — done
- [t-250914231210-dsg03] Docs: load tokens via `extra_css`; add Design Theme page — done
- [t-250914231215-dsg04] Launcher: adopt tokens across common/index/events/logs/models/connections — done (initial)
- [t-250914231220-dsg05] Service Debug UI: replace inline styles with token vars — in progress
- [t-250914231225-dsg06] Docs overrides: dedupe variables; rely on tokens only — done
- [t-250914231240-dsg09] CI: add tokens sync check step (uses `just tokens-check`) — done
- [t-250914231230-dsg07] Extract `ui-kit.css` primitives (buttons/inputs/badges) for launcher pages — done
- [t-250914231235-dsg08] Contrast audit (WCAG AA) sweep; adjust any low‑contrast cases — todo
- [t-250914231245-dsg10] Add W3C tokens pipeline (Style Dictionary) to emit platform targets — plan
- [t-250914231250-dsg11] Add prefers-contrast / forced-colors styles for key components — todo
 - [t-250914231255-dsg12] Tailwind tokens export (JSON) for downstream configs — done

Standards & Docs
- [t-250914231255-std01] Add ADR framework and seed first ADRs (tokens SSoT, event naming) — plan
 - [t-250914231300-std02] Optional docs a11y check in CI (axe) — done

Recipes MVP
- Finalize `spec/schemas/recipe_manifest.json`; add doc‑tests and example gallery entries
- Gallery UI: install (copy folder), inspect manifest, launch, capture Episodes
- Permission prompts (ask/allow/never) with TTL leases; audit decisions as `Policy.*` events

Logic Units (High Priority)
- Manifest and schema (`spec/schemas/logic_unit_manifest.json`); example units under `examples/logic-units/`
- Library UI: tabs (Installed/Experimental/Suggested/Archived), diff preview, apply/revert/promote
- Agent Profile slots + compatibility checks (design + stubs)
- A/B dry‑run pipeline wired to Evaluation Harness; per‑unit metrics panel
- Research Watcher: ingest feeds; draft config‑only units; Suggested tab source

Last‑mile Structures
- Config Patch Engine: dry‑run/apply/revert endpoints; schema validation; audited permission widening
- Experiment Orchestrator: start/stop/assign; emit Experiment.*; fold to `/state/experiments`
- Provenance/Snapshots: enrich `/state/episode/{id}/snapshot` with effective config (units, params, model hash, policies)
- AppSec Harness: seed tests; surface violations as `policy.decision` events; block unsafe tool I/O
- Observability (OTel): map timeline to traces (corr_id as trace); correlate metrics/logs
- Compliance Mode: workspace switch; record‑keeping + approvals; UI status widget
- Supply‑Chain Trust: signed manifests, SBOMs, sandbox defaults; align desktop capabilities with policies
- Scheduler/Governor: fair queues, preemption, backpressure, kill‑switch; policy‑aware

Security & Admin
- Admin auth hardening — hashed tokens + per‑token/IP sliding rate‑limit [t-250911230312-0863]
- Per‑route gating layers; slim global admin middleware [t-250911230252-9858]
- Supply‑chain: upgrade GTK/GLib stack to >=0.20 (via wry/gtk/tao/tauri) to resolve RUSTSEC-2024-0429; remove temporary ignore in `deny.toml` and audit script guard once lockfile carries `glib >= 0.20.0`.

Caching & Performance (High Priority)
- [t-250913001000-1001] Llama.cpp prompt cache: set `cache_prompt: true` in requests; doc server `--prompt-cache` for persistence — in progress
- [t-250913001003-1002] CAS HTTP caching: add `ETag`, `Last-Modified`, long‑lived `Cache-Control`, and 304 handling to `/admin/models/by-hash/{sha256}` — done
- [t-250913001006-1003] Action Cache (MVP): wrap `tools_exec::run` with deterministic key (tool id, version, canonical JSON, env signature stub) and CAS’d outputs; Moka front with TTL; `tool.cache` events — in progress
- [t-250913001009-1004] Singleflight: coalesce identical in‑flight tool runs and expensive read‑model recomputes — todo
- [t-250913001012-1005] Read‑models SSE deltas: stream JSON Patch with `Last-Event-ID` resume; wire Debug UI to apply patches — todo
- [t-250913001015-1006] Metrics: expose cache hit/miss/age, bytes/latency saved, stampede suppression rate at `/state/*` and `/metrics` — todo
- [t-250913001018-1007] Cache Policy manifest + loader: define YAML format, map to env knobs, and plan migration to config‑first overrides — docs done; loader todo
- [t-250914210100-http01] HTTP helpers module (`ext::http`) for ETag/Last‑Modified/Range parsing; adopt in models blob GET/HEAD — done
- [t-250914210104-http02] Adopt `ext::http` helpers across any future digest/static file endpoints (keep semantics consistent) — todo
- [t-250914210107-http03] Docs: consolidate HTTP caching semantics into a short reusable snippet and cross‑link from API/Guide — done

Egress Firewall & Posture (Plan)
- Policy: add network scopes (domain/IP/CIDR, port, protocol) and TTL leases; surface in UI.
- Gateway: per‑node loopback proxy (HTTP(S)/WS CONNECT; optional SOCKS5) with allow/deny by SNI/Host and port; deny IP‑literals by default; no TLS MITM.
- DNS Guard: force resolver, block UDP/53/DoH/DoT from tools; short TTLs; log lookups.
- Routing: containers via proxy env + blocked direct egress; host processes with OS firewall rules (allow 127.0.0.1:proxy only for agent PIDs).
- Ledger: append‑only egress ledger with episode/project/node attribution and bytes/$ estimates; pre‑offload preview dialog in UI.
- Posture: Off/Public/Allowlist/Custom per project; default to Public.
- Filesystem & Sensors: sandbox write scopes (project://) and leased mic/cam sidecar access with TTL + audits.
- Cluster: replicate gateway+DNS guard per Worker; propagate policies from Home over mTLS; Workers cannot widen scope.

Lightweight Mitigations (Plan)
- Memory quarantine: add review queue and `memory.quarantined`/`memory.admitted` events; admit only with provenance + evidence score.
- Project isolation: enforce per‑project namespaces for caches/embeddings/indexes; “export views” only; imports read‑only and revocable.
- Belief‑graph ingest: queue world diffs; surface conflicts; require accept/apply with audit events.
- Cluster manifest pinning: define signed manifest schema; publish/verify; scheduler filters to trusted manifests.
- Secrets hygiene: vault‑only; redaction pass on snapshots/egress previews; secret‑scanner job for artifacts.
- Hardened headless browsing: disable service workers/HTTP3; same‑origin fetches; DOM‑to‑text extractor; route via proxy.
- Safe archive handling: temp jail extraction with path canonicalization; size/time caps; nested depth limit.
- DNS guard + anomaly: rate‑limit lookups; alert on high‑entropy domain bursts.
- Accelerator hygiene: zero VRAM/buffers between jobs; disable persistence mode where possible; prefer per‑job processes.
- Co‑drive role separation: roles view/suggest/drive; “drive” cannot widen scope or approve leases; tag remote actions.
- Event integrity: mTLS; nonces; monotonic sequence numbers; idempotent actions; reject out‑of‑order/duplicates.
- Context rehydration guard: redaction/classification before reuse in prompts; badge “potentially exportable”; require egress lease if offloaded.
- Operational guardrails: per‑project security posture (Relaxed/Standard/Strict); egress ledger retention + daily review UI; one‑click revoke.
- Hygiene cadence: quarterly key rotation & re‑sign; monthly dependency sweep with golden tests & snapshot diffs.
- Seeded red‑team tests in CI: prompt‑injection, zip‑slip, SSRF, secrets‑in‑logs detector.

Remote Access & TLS
- Dev TLS profiles (mkcert + self‑signed) for localhost
- Caddy production profile with Let's Encrypt (HTTP‑01/DNS‑01) for public domains
- Reverse‑proxy templates (nginx/caddy) with quick run/stop helpers
- Secrets handling: persist admin tokens only to local env files; avoid committing to configs
- Setup wizards to pick domain/email, validate DNS, and dry‑run cert issuance

Observability & Eventing
- Event journal: reader endpoint (tail N) and topic‑filtered consumers across workers/connectors
- Metrics registry with histograms; wire to /metrics [t-250911230320-8615]
- Docs: surface route metrics/events in docs and status page — done
- RPU trust: watcher + endpoints + `rpu.trust.changed` event + Prometheus gauges — done

Compatibility & Hardware
- GPU probe fallback via wgpu: enumerate adapters across backends — done

State Read‑Models & Episodes
- Observations read‑model + GET /state/observations [t-250912001055-0044]
- Beliefs/Intents/Actions stores + endpoints [t-250912001100-3438]
- Episodes + Debug UI reactive views (truth window) [t-250912001105-7850]
- Debug UI: Episodes filters + details toggle [t-250912024838-4137]

Hierarchy & Governor Services
- Encapsulate hierarchy (hello/offer/accept/state/role_set) and governor (profile/hints) into typed services; endpoints prefer services; publish corr_id events; persist orchestration [t-250912024843-7597]

CLI & Introspection
- Migrate arw-cli to clap (derive, help, completions, JSON flag) [t-250911230329-4722]
- Auto‑generate /about endpoints from router/introspection [t-250911230306-7961] — done
  - /about merges public endpoints (runtime recorder) and admin endpoints (macro registry); entries are `METHOD path`; deduped and sorted.

Queues, NATS & Orchestration
- Orchestrator: lease handling, nack(retry_after_ms), group max in‑flight + tests [t-250911230308-0779]
- NATS: TLS/auth config and reconnect/backoff tuning; docs and examples [t-250911230316-4765]

Specs & Docs
- Generate AsyncAPI + MCP artifacts and serve under /spec/* [t-250909224102-9629]
- Docgen: gating keys listing + config schema and examples
- AsyncAPI: include `rpu.trust.changed` channel — done
- Event normalization rollout
  - [t-250913213500-ev01] Add dual-mode warning logs when ARW_EVENTS_KIND_MODE=dual to surface remaining legacy consumers — removed (modes dropped)
  - [t-250913213501-ev02] Update all docs/screenshots/snippets to normalized kinds (models.download.progress, …) — in progress
  - Gallery guide and screenshots guide updates — done
  - [t-250913213502-ev03] Add envelope schema `ApiEnvelope<T>` in OpenAPI and adopt in responses (opt-in) — todo
  - [t-250913213503-ev04] Add short descriptions to any endpoints with Spectral hints (e.g., /state/models) — done
  - [t-250913213504-ev05] Adjust Spectral AsyncAPI rule to accept dot.case explicitly to remove naming warnings — todo
  - [t-250913213505-ev06] Plan removal: switch all deployments to normalized kinds, then drop legacy/dual paths — done
  - [t-250913213506-ev07] Add deprecation note to release notes (legacy event kinds) — done

Visual Capture (Screenshots)
- Screenshot tool (OS‑level): capture entire screen/display or region; save to `.arw/screenshots/` and emit `screenshots.captured` — done
- Window crop: obtain Tauri window bounds and crop screenshot to window rectangle — done
- OCR pass (optional): run OCR over captured region (tesseract/rapidocr) to extract text for search — in progress (feature‑flag ready; enabled by default)
- Sidecar: Activity lane shows recent screenshots as thumbnails with open/copy actions — done
- Annotation: overlay + burn tool with sidecar JSON; blur+border — done
- Gallery: modal with actions and annotate — done
- Policy: gate under `io:screenshot` and `io:ocr`; leases and audit — done
- Next: redaction presets; labels/arrows; retention and search in gallery; Save to project macro to append Markdown in NOTES.md.

Strict dot.case normalization (no back-compat)
- [t-250914050900-ev10] Update topics SSoT to dot.case only (remove CamelCase constants) — in progress
- [t-250914050902-ev11] Replace all publishers to use topics.rs constants (no hard-coded strings) — in progress
- [t-250914050904-ev12] Debug UI: switch listeners to dot.case only — todo
- [t-250914050906-ev13] Connector: publish `task.completed` and subjects `arw.events.task.completed` + node variant — todo
- [t-250914050908-ev14] Update Feature Matrix topics to dot.case — in progress
- [t-250914050910-ev15] Docs: update Events Vocabulary/Topics/Admin Endpoints to dot.case — todo
- [t-250914050912-ev16] CI linter: fail on any `publish("...CamelCase...")` or legacy subjects — todo
- [t-250914050914-ev17] arw-core gating keys: add `events:task.completed` and update callers — plan
- [t-250914050916-ev18] Release notes: breaking change, mapping table, migration notes — todo

OpenAPI/Examples
- [t-250913213507-api01] Add examples for /admin/models/jobs and /admin/models/download responses — done
- [t-250913213508-api02] Document public /state/models envelope explicitly or add note about envelope omission in examples — todo

Feedback Engine (Near‑Live)
- Engine crate and integration: actor with O(1) stats, deltas via bus, snapshot+persistence [t-250909224102-8952]
- UI: near‑live feedback in /debug showing deltas with rationale/confidence [t-250909224103-0211]
- Policy hook: shadow → policy‑gated auto‑apply with bounds/rate‑limits [t-250909224103-5251]

Testing
- End‑to‑end coverage for endpoints & gating; fixtures; CI integration [t-250911230325-2116]
- [t-250914210200-test01] Scoped state dir for tests (`test_support::scoped_state_dir`) to isolate `ARW_STATE_DIR` — done
- [t-250914210204-test02] Migrate env‑derived `state_dir` lookups to a process‑lifetime cache (OnceCell) with a test‑only reset hook to avoid flakiness — todo
- [t-250914210208-test03] Concurrency controls: add tests for `block=false` shrink path and pending_shrink reporting — todo

Stabilization & Contracts
- [t-250914210300-api01] Models summary: switch handler to typed snapshots from `ModelsService` (avoid ad‑hoc JSON picks) — done
- [t-250914210302-api06] Concurrency: expose `pending_shrink` in `GET /admin/models/concurrency` and jobs snapshot — done
- [t-250914210303-ui01] Models UI: add Jobs panel, concurrency controls with feedback, Installed Hashes filters + reset, and persistence — done
- [t-250914210304-ui02] Installed Hashes: add pagination controls (offset/next/prev) — todo
- [t-250914210304-api02] Egress ledger: add compact summarizer endpoint (time‑bounded; filters by decision/reason) — done
- [t-250914210308-api03] CAS GC: centralize manifest reference extraction (single helper used by GC and any scanners) — todo
- [t-250914210312-api04] Contract tests: promote spec/docs consistency checks to CI (status/code enums already validated in unit tests) — todo
- [t-250914210316-api05] Sweep and remove any remaining deprecated endpoints; update docs and SDKs — todo

## Next (1–2 Months)

Platform & Policy
- WASI plugin sandbox: capability‑based tool permissions (ties to Policy)
- Policy engine integration (Cedar bindings); per‑tool permission manifests
- RPU: trust‑store watch + stronger verification; introspection endpoint [t-250911230333-9794]

Models & Orchestration
- Model orchestration adapters (llama.cpp, ONNX Runtime) with pooling and profiles
- Capsules: record inputs/outputs/events/hints; export/import; deterministic replay

Specs & Interop
- AsyncAPI + MCP artifacts in CI; promote developer experience around /spec/*

Docs & Distribution
- Showcase readiness: polish docs, packaging, and installer paths

## Notes
- For live status of all tracked tasks, see Developer → Tasks, which renders from `.arw/tasks.json`.
- Recently shipped work is summarized under About → Roadmap → Recently Shipped.
