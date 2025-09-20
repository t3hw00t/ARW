# Changelog

This project follows Keep a Changelog and Semantic Versioning. All notable changes are recorded here.

## [Unreleased]

- Added lease-aware capsule adoption (`policy.capsule.expired` telemetry, read-model patches) and tightened guardrail gateway refresh behaviour.
- Restored unified server chat endpoints (`/admin/chat*`) with debug UI panels for staging approvals, research watcher, and training telemetry.
- Retired legacy `/memory/*` REST shims and the `/admin/events` alias; new admin helpers live at `/admin/memory/*` and SSE streams at `/events`.
- Regenerated OpenAPI/JSON artifacts to reflect the updated surface and removed legacy routes.

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
- `arw-svc` tests pass (53/53). Local builds verified for Docker/Helm paths.

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
- Bumped `arw-svc` to `0.1.3`.

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
- Feature-flagged gRPC server for `arw-svc` (opt-in via `--features grpc` and `ARW_GRPC=1`).
- Windows script improvements + Pester tests; CI job to run them.
- CI: cargo-audit, cargo-deny, Nix build/test job, docs link-check (lychee), CodeQL.
- Helm chart for `arw-svc` with readiness/liveness probes.
- Docker: multi-stage image, non-root runtime; Compose file and Justfile helpers.
- Devcontainer (Nix) for consistent dev environment.
- Docs: Training research, Wiki structure, gRPC guide; stability freeze checklist.

### Changed
- Consolidated merged branches; pruned stale `codex/*` remotes.
- Introduced Tauri-based launcher (`arw-launcher`) and aligned scripts to a launcher‑first flow.
- `arw-cli` updated to rand 0.9.
- `arw-svc` refactors: AppState/Resources split; extended APIs and Debug UI.
- CI installs Tauri/WebKitGTK deps on Linux for launcher builds.

### Fixed
- Clippy lints in macros and service; formatting across touched files.

### Security
- Added CodeQL analysis and cargo-audit.
- Helm securityContext defaults; non-root Docker image.
