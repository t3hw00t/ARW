# Rapid Iteration Guardrails
Updated: 2025-10-01
Type: Reference

We operate with short iteration cycles and track the latest stable Rust toolchain (currently 1.90+). This page captures the lightweight guardrails that keep ARW broadly compatible across platforms while we move fast.

## Scope
- Core crates and binaries: `arw-protocol`, `arw-events`, `arw-core`, `arw-otel`, `arw-server`, `arw-cli`, `arw-connector`
- Desktop surfaces (`arw-tauri`, `arw-launcher`) follow the same policies but can experiment behind feature flags
- CI surfaces: build, test, clippy, deny/audit, docs + link checks, packaging scripts

## Guardrails (keep these true even during rapid changes)
- HTTP and SSE contracts remain backward compatible inside a release train; additive changes are welcome, breaking ones need a clear migration plan
- Tool IDs stay semver’d and policy-gated; ship migrations alongside schema changes
- Docs (`mkdocs build --strict`) and generated specs stay current with code
- Clippy remains clean (`-D warnings`) on the latest stable toolchain
- Cross-platform scripts remain runnable on macOS/Linux/Windows shells

## Fast Release Loop
- Format: `cargo fmt --all -- --check`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Build: `cargo build --workspace --locked`
- Test: `cargo test --workspace --locked`
- Specs & docs: `just openapi-gen` and (`bash scripts/docgen.sh` / `powershell -ExecutionPolicy Bypass -File scripts\docgen.ps1`) followed by `mkdocs build --strict`

## Desktop Launcher Notes
- Tauri 2 capabilities live at `apps/arw-launcher/src-tauri/capabilities/main.json`
- Permissions allowlist: `apps/arw-launcher/src-tauri/permissions/arw.json`
- Enable new commands by updating the allow list; unused commands are stripped when `build.removeUnusedCommands: true`
- Security hygiene: `cargo audit` and `cargo deny check advisories bans sources licenses`

## Versioning Guidance
- Patch: bug fixes, doc updates, internal refactors
- Minor: additive APIs/events/tools, new feature flags
- Major: intentional contract breaks; announce early and document migrations

## Branch & Review Flow
- Prefer short-lived branches; keep CI green before merge
- Avoid force pushes on `main`; fast-forward merges are fine when CI results are reused
- Call out follow-up tasks or breakpoints in PR descriptions when shipping stepping stones

## Observability & Diagnostics
- Emit structured logs (`key=value`), especially around orchestrator decisions
- Continue publishing `service.*` lifecycle events so dashboards stay accurate
- Update Grafana and alert templates when telemetry fields change

## Move-Fast Playbook
- When adding high-impact features, stage with feature flags or environment toggles so rollbacks remain easy
- If a change requires new platform dependencies, update install docs and scripts in the same PR
- Use `cargo msrv verify` as a “latest stable sanity check” whenever bumping toolchains or adding nightly-only suggestions; expect it to pass on the newest stable channel
