# Changelog

This project follows Keep a Changelog and Semantic Versioning. All notable changes are recorded here.

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
- `arw-tray` updated to ureq v3; `arw-cli` to rand 0.9.
- `arw-svc` refactors: AppState/Resources split; extended APIs and Debug UI.
- CI excludes desktop UI crates from default Linux builds for stability.

### Fixed
- Clippy lints in macros and service; formatting across touched files.

### Security
- Added CodeQL analysis and cargo-audit.
- Helm securityContext defaults; non-root Docker image.
