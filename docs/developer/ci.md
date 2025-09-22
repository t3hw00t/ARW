---
title: CI & Releases
---

# CI & Releases

Updated: 2025-09-22
Type: Reference

## Continuous Integration
- Build and test on Linux and Windows for every push and PR.
- Lint and format checks keep changes tidy.

### Interfaces workflow (APIs, events, tools)
- Lint: Spectral on `spec/openapi.yaml` and `spec/asyncapi.yaml` using `quality/openapi-spectral.yaml`.
- Diff: OpenAPI via `tufin/oasdiff` (breaking changes fail), AsyncAPI via `@asyncapi/diff` (markdown artifact).
- Sync: Generate OpenAPI from code (`OPENAPI_OUT=spec/openapi.yaml cargo run --no-default-features -p arw-server`) and normalize‑diff against `spec/openapi.yaml`.
- Mock: Boot Prism on OpenAPI and smoke a request.
- Hygiene: fail if any descriptor has `review_after` or `sunset` past due.
- Docs: generate “Interface Deprecations” and attach “Interface Release Notes” as artifacts.

## Artifacts
- Packaging scripts assemble a portable bundle with binaries and configs.
- Windows and Linux bundles are uploaded as CI artifacts.

### Windows Launcher Bundles
- Workflow: `.github/workflows/tauri-windows.yml` builds launcher MSIs via a two‑arch matrix (x64 primary; ARM64 best‑effort) and uploads them with svc/cli.
- MSI content: includes `arw-server.exe` and `arw-cli.exe` so service autostart works out‑of‑the‑box.
- Optional code signing: enable by adding `WINDOWS_CERT_PFX` (base64 PFX) and `WINDOWS_CERT_PASSWORD` secrets; artifacts are signed with `signtool`.
- Release: on tagged pushes (`v*.*.*`), x64 MSI always publishes; ARM64 MSI publishes when the toolchain supports cross‑bundling.
- Smoke test: x64 only — installs the MSI on the runner, verifies files, launches briefly, polls `/healthz`, then uninstalls (non‑blocking).

## Docs Site
- Docs are published to GitHub Pages from the `gh-pages` branch.

## Local Helpers
```powershell
# Build & test
scripts/build.ps1
scripts/test.ps1

# Package bundle (release)
scripts/package.ps1

# Quick debug run (service with /admin/debug)
scripts/debug.ps1

# Supply-chain audit (cargo-audit + cargo-deny)
scripts/audit.ps1
```

## Local CI Mirror
Run the same checks as CI locally:

```bash
# In repo root
cd Agent_Hub

# 1) Build, lint, test
cargo build --workspace --all-targets --locked
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# 2) Supply-chain checks (advisories/licenses/sources/bans)
cargo install cargo-deny --locked # once
cargo deny check advisories bans sources licenses || true

# Or use the helper wrapper (Bash/PowerShell):
scripts/audit.sh --interactive

# 3) Links (README + docs)
cargo install lychee --locked # once
lychee --no-progress --config .lychee.toml README.md docs/**

# 4) Docs build
python3 -m venv .venv && . .venv/bin/activate
pip install mkdocs-material mkdocs mkdocs-git-revision-date-localized-plugin
mkdocs build --strict

# 5) Interfaces (local)
just interfaces-index       # regenerate interfaces/index.yaml
just interfaces-lint        # spectral lint OpenAPI/AsyncAPI
just interfaces-diff        # OpenAPI diff vs origin/main (Docker)
just docs-deprecations      # generate deprecations doc
just docs-release-notes     # generate release notes (BASE_REF=... override)
just check-enums            # verify ModelsDownloadProgress status/code enums match code

# Design tokens (single source)
just tokens-sync            # copy assets/design tokens to docs and launcher UI
just tokens-check           # verify synced copies match single source
just tokens-build           # regenerate CSS/JSON tokens from W3C tokens
just tokens-rebuild         # build + sync + check tokens (SSoT)
```

Tips
- Set `GITHUB_TOKEN` when running `lychee` to reduce GitHub rate limits.
- On Debian/Ubuntu, use a virtualenv to avoid PEP 668 errors when installing MkDocs.

## Additional Checks
```bash
# Unused dependencies
rustup toolchain install nightly --profile minimal
cargo +nightly install cargo-udeps --locked
cargo +nightly udeps --workspace --all-targets

# Verify MSRV
cargo install cargo-msrv --locked
cargo msrv verify

# Event naming (dot.case)
python3 scripts/lint_event_names.py
```
