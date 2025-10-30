# Changelog

This project follows Keep a Changelog and Semantic Versioning. All notable changes are recorded here.

## [Unreleased]

### Added
- Adapter manifest lints in SDK with unit tests.
- CLI `arw-cli adapters validate` to lint manifests (human/JSON, strict mode).
- CLI `arw-cli adapters schema` to generate JSON Schema.
- CLI `arw-cli adapters init` to scaffold a new manifest (JSON/TOML).
- Hosted JSON Schema and docs site copy; VS Code schema mapping.
- Documentation: how-to guide, reference, schema index, and links in runtime matrix.
- Sample manifests (JSON and TOML) under `examples/adapters/` and CI examples under `adapters/`.
- CI: adapters lint workflow with PR fast-path, strict mode, summary table, and annotations.
- Dev UX: Justfile helpers, optional verify integration, pre-commit hook.

### Changed
- Config watcher loops now back off exponentially and surface a single warning when runtime configs are missing, reducing log noise until files appear.
- Context cascade defaults were trimmed (2 K events / 512 per episode) to tame memory pressure on low-spec hosts; docs call out the env overrides when wider windows are needed.
- Egress proxy restarts await graceful shutdown before rebinding, avoiding transient “address already in use” errors during reloads.
- README: added Adapters Guide badge and links.

### Housekeeping
- MkDocs nav includes the persona preview, empathy research plan, DeepSeek OCR pipeline, and maintenance guides so preview surfaces are easier to discover.
- `.gitignore` now ignores local `apps/arw-server/state/` artifacts along with transient server logs, keeping the worktree clean after local runs.
- Retired all published bundles through `v0.1.4`, refreshed the workspace to the `0.2.0-dev` pre-release train, and removed legacy artifacts from the tree.
- Hardened bus fan-out to survive broadcast lag (SSE replay, metrics, and state observers now recover after channel overflows) and tightened filtered subscriber logging.
- Auto-scale the action worker pool (defaults to roughly 2× host cores, configurable via `ARW_WORKERS` with an optional `ARW_WORKERS_MAX` cap) and added SQLite indexes for the actions table to keep queue drains fast as workloads grow.
- Memory overlay persists embeddings in both JSON and a new binary `embed_blob` column; a background backfill task (`ARW_MEMORY_EMBED_BACKFILL_BATCH`) upgrades legacy rows in place, surfaces progress metrics, and keeps hybrid/vector searches from reparsing floats.
- Exposed worker/queue metrics (`arw_workers_*`, `arw_actions_queue_depth`) across `/state/route_stats` and Prometheus output for easier capacity tuning.
- Added lease-aware capsule adoption (`policy.capsule.expired` telemetry, read-model patches) and tightened guardrail gateway refresh behaviour.
- Restored unified server chat endpoints (`/admin/chat*`) with debug UI panels for staging approvals, research watcher, and training telemetry.
- Retired legacy `/memory/*` REST shims and the `/admin/events` alias; new admin helpers live at `/admin/memory/*` and SSE streams at `/events`.
- Regenerated OpenAPI/JSON artifacts to reflect the updated surface and removed legacy routes.
- Removed the legacy `X-ARW-Gate` capsule header; requests must send capsules with `X-ARW-Capsule` (legacy usage now returns HTTP 410 and emits failure telemetry).
- Launcher sidecar: accessible lane toggles, smarter lane-aware subscriptions/cleanup, EventSource stale detection, and a dedicated `sidecar.js` module with expanded tests keep UI state responsive even with customised layouts.
- Launcher sidecar approvals lane: initialise sort/filter state defensively, avoid post-dispose render crashes, and extend tests to cover approvals workflows.
- Context loop and memory packing now reuse shared `Arc` snapshots for beliefs and batch linked-memory lookups, slashing blocking-lane clones and SQLite round-trips; SQLite helpers expose `get_memory_many` plus record-returning inserts so callers can emit events without extra queries. Working-set selection also precomputes slot labels and caches slot limits to reduce per-iteration string churn.
- Hybrid memory retrieval now honours the caller’s requested limit (instead of always pulling 400 rows) and fast-path parses `updated` timestamps, trimming SQLite and chrono overhead during memory searches.
- Working-set candidate selection switched to a lazy recomputing heap that avoids O(n²) rescoring, yielding lower CPU cost when context expansion returns large candidate sets.
- Kernel hot paths now use rusqlite’s statement cache for event and action mutations, eliminating redundant SQL recompilation while holding existing pool semantics intact.
- Queue wakeups avoid SeqCst fences and polling sleeps; access logs capture only the required headers and stream through `tracing`, reducing scheduler wakeups and stdout backpressure.
- Tool cache hashing streams canonical JSON directly into the digest, avoiding large intermediate buffers when caching sizable tool inputs.
- Kernel async helpers now ride a dedicated blocking pool (tunable via `ARW_KERNEL_BLOCKING_THREADS`), replacing ad-hoc `spawn_blocking`, smoothing rusqlite latency tails, and exporting queue-depth/counter metrics for observability.
- Preview persona tooling: added `arw-cli admin persona seed` (with `just persona-seed` wrapper) and documented the [Persona Preview Quickstart](docs/guide/persona_quickstart.md) so operators can enable `ARW_PERSONA_ENABLE` without writing SQL; related guides now flag persona surfaces as preview-only and link to the helper.

## [0.1.4] - 2025-09-15

### Added
- Request correlation: `x-request-id` middleware adds/echoes request IDs across responses and traces.
- Security headers: `X-Content-Type-Options`, `Referrer-Policy`, `X-Frame-Options`, and conservative `Permissions-Policy` applied to all responses.
- CSP controls: auto‑inject CSP for HTML (toggle via `ARW_CSP_AUTO`), presets (`ARW_CSP_PRESET=relaxed|strict`), and explicit override (`ARW_CSP`). Applied to Landing, Admin Index, and Specs pages.
- Server‑Timing: `Server-Timing: total;dur=<ms>` for quick client‑side latency checks.
- Proxy‑aware IP: rate‑limits prefer the remote socket; honor `X‑Forwarded‑For` only when `ARW_TRUST_FORWARD_HEADERS=1`.
- Structured access logs: `http.access` target with sampling and fields (method, path, matched, status, dt_ms, ip, request_id, req/resp lengths). Optional UA/Referer (hash/strip QS) via env.
- Rolling access logs: file rotation with `tracing-appender` (daily/hourly/minutely) gated by `ARW_ACCESS_LOG_ROLL=1`; directory/prefix/rotation configurable.
- SSE correlation: initial `service.connected` includes `request_id`; optional per‑event decoration (`ARW_EVENTS_SSE_DECORATE=1`) adds request id to payloads (envelope and CloudEvents).
- Docker: healthcheck, copies `spec/` and `interfaces/`; improved runtime deps; README Docker quickstart.
- Helm: chart values expose CSP, proxy trust, structured + rolling access log envs; deployment template updated.
- Ops: systemd unit examples (native and container) and docs.
- Docs: new/updated guides for Docker, Kubernetes (Helm), Reverse Proxy, Security Hardening (headers/CSP/logs), Systemd Service.
- Dev: `just access-tail` to follow latest rolled `http.access` file.
- CI: GitHub Actions workflow to build and publish multi‑arch Docker images to GHCR (`docker-publish.yml`).

### Changed
- Trace spans include request id and client IP; improved admin/spec/landing HTML structure and themes.
- Compose defaults expose `ARW_BIND`; chart defaults set `ARW_BIND=0.0.0.0` (override behind proxies).

### Security
- Safer defaults for response headers and CSP; proxy header trust is opt‑in.

### Testing
- `arw-server` tests pass (53/53). Local builds verified for Docker/Helm paths.

## [0.1.3] - 2025-09-15

### Added
- Canonical Admin UI routes: `/admin/debug` and `/admin/ui/{models,agents,projects}`.
- Shared design system (tokens + UI kit) served under `/admin/ui/assets/*` and adopted across service pages.
- Remote windows (Events/Logs/Models) with base-aware Tauri proxy commands; header injection avoids CORS.
- Launcher SSE robustness: auto-reconnect with modest backoff, replay/prefix preservation, and connection status events.
- Section toolbars and SSE badges on service pages; global `.card` spacing via UI kit.

### Changed
- Unified badge tones to `ok/warn/bad`; removed page-local badge/dot styles; replaced radial backgrounds with tokenized surfaces.
- Debug containers converted to `.card`; action rows use ARIA `role="toolbar"`.

### Fixed
- Resolved overlapping `/debug` route alias; kept intended debug-only aliases.
- Docs warnings fixed (Updated/Generated metadata, title case); strict docs build clean.

### Developer
- Bumped `arw-tauri` to `0.1.3`; launcher depends on the new plugin.
- Bumped `arw-server` to `0.1.3`.

## [0.1.2] - 2025-09-13

### Changed
- Launcher-first start flow across scripts and interactive menus.
- Removed legacy tray binary from packaging and setup; migrated docs to launcher terminology.
- Linux CI and dist workflows install Tauri/WebKitGTK deps for launcher builds.

### Added
- Launcher tray menu grouped into Service / Debug / Windows / Quit; live health polling and notifications.

## [0.1.0-beta] - 2025-09-11

Stability baseline. Consolidated features, CI hardening, docs, and ops.

### Added
- Feature-flagged gRPC server (opt-in via `--features grpc` and `ARW_GRPC=1`).
- Windows script improvements + Pester tests; CI job to run them.
- CI: cargo-audit, cargo-deny, Nix build/test job, docs link-check (lychee), CodeQL.
- Helm chart with readiness/liveness probes.
- Docker: multi-stage image, non-root runtime; Compose file and Justfile helpers.
- Devcontainer (Nix) for consistent dev environment.
- Docs: Training research, Wiki structure, gRPC guide; stability freeze checklist.

### Changed
- Consolidated merged branches; pruned stale `codex/*` remotes.
- Introduced Tauri-based launcher (`arw-launcher`) and aligned scripts to a launcher‑first flow.
- `arw-cli` updated to rand 0.9.
- Service refactors: AppState/Resources split; extended APIs and Debug UI.
- CI installs Tauri/WebKitGTK deps on Linux for launcher builds.

### Fixed
- Clippy lints in macros and service; formatting across touched files.

### Security
- Added CodeQL analysis and cargo-audit.
- Helm securityContext defaults; non-root Docker image.
