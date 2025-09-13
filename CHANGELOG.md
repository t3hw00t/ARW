# Changelog

This project follows Keep a Changelog and Semantic Versioning. All notable changes are recorded here.

## [Unreleased]

_No changes yet._

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
