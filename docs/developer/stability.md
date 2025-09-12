# Stability Freeze Checklist

This project is in a stability/consolidation phase. Use this checklist before
adding new features.

## Scope (what we stabilize now)
- Core crates: `arw-protocol`, `arw-events`, `arw-core`, `arw-otel`
- Binaries: `arw-svc`, `arw-cli`, `arw-connector`
- CI: build/test, clippy, audit, deny, docs+link-check
- Docs: generated specs, guides, and nav

Desktop UI crates (`arw-tauri`, `arw-launcher`) are kept out of Linux CI builds
and can evolve independently while core stabilizes.

## Invariants
- HTTP surface remains compatible (OpenAPI regenerates without breaking changes)
- SSE event kinds remain stable (new events append-only)
- Tool IDs are semver’d and gated by policy
- Docs build cleanly (`mkdocs build --strict`)
- Clippy passes on core crates with `-D warnings`

## Release checklist
- Format: `cargo fmt --all -- --check`
- Lint: `cargo clippy -p arw-protocol -p arw-events -p arw-core -p arw-macros -p arw-cli -p arw-otel -p arw-svc -p arw-connector --all-targets -- -D warnings`
- Build: `cargo build --workspace --locked --exclude arw-tauri --exclude arw-launcher`
- Test: `cargo test --workspace --locked --exclude arw-tauri --exclude arw-launcher`
- Security: `cargo audit`; `cargo deny check advisories bans sources licenses`
- Spec: `OPENAPI_OUT=spec/openapi.yaml cargo run -p arw-svc`
- Docs: `bash scripts/docgen.sh && mkdocs build --strict`

## Versioning
- Patch: docs, internal improvements, non-breaking changes
- Minor: new endpoints/events/tools; additive changes only
- Major: any breaking change (avoid during stabilization)

## Branch policy
- Work from short-lived topic branches; PRs must be green on CI
- Avoid force-push on `main`; use PR merge (fast-forward or merge commit)

## Observability
- Prefer structured logs (key=value) and consistent error variants
- Emit `Service.*` lifecycle events around startup/shutdown

## When to unfreeze
- CI is green on multiple runs
- Recent changes sit without regressions for a few days
- Docs published and verified (links OK)

