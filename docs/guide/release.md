---
title: Release Guide
---

# Release Guide

Updated: 2025-10-09
Type: Runbook

This runbook captures the end-to-end process for cutting an Agent Hub (ARW) release. It focuses on the unified `arw-server` surface, the CLI, docs, and portable bundles. Windows installers and other follow-on packaging inherit from these steps.

## Prerequisites
- Rust toolchain `1.90` (see `rust-toolchain.toml`) with `cargo`, `just`, and Python 3 available.
- Access to the main repository with permission to push tags.
- GitHub token (`GH_TOKEN` or `GITHUB_TOKEN`) that can read issues and publish releases.
- Clean workspace with release notes drafted (`CHANGELOG.md`, `RELEASE_NOTES_*`, docs updates).
- CI green on `main` or the branch you intend to tag.

## Preflight Checklist
1. **Backlog review** – confirm there are no open release blockers. Use `scripts/check_release_blockers.sh` (requires GitHub token).
2. **Version bump** – update `workspace.package.version` in the root `Cargo.toml` (and any files that pin the version such as `apps/*/Cargo.toml`).
3. **Docs & notes** – finalize `CHANGELOG.md`, release notes, and update docs for any breaking or preview changes.
4. **Dependency audit** – ensure `cargo deny` or equivalent checks have already run in CI; resolve any `deny.toml` ignores you can remove.
5. **Installer status** – confirm Windows/Linux installers or bundles referenced in docs have matching versions queued.

> Tip: when iterating on documentation, run `just docs-check` early to catch broken links and heading regressions.

## Validation & Linting

Run the standard quality gates from repo root:

```bash
just fmt-check
just lint
just test
just runtime-smoke
```

`just runtime-smoke` exercises the managed runtime supervisor. On machines without model weights or GPU access it falls back to the stub path; set `ARW_SMOKE_USE_SYNTHETIC=1` if sockets are unavailable. Add extra smoke runs (`just runtime-smoke-gpu`, `just triad-smoke`) when hardware is present.

For targeted crates you can use the release script’s set:

```bash
cargo clippy -p arw-protocol -p arw-events -p arw-core -p arw-macros \
  -p arw-cli -p arw-otel -p arw-server -p arw-connector --all-targets -- -D warnings
cargo test --workspace --locked --exclude arw-tauri --exclude arw-launcher
```

## Docs & Specs Regeneration

Regenerate interface specs, gating references, and API docs so they match what ships:

```bash
just docgen   # wraps scripts/docgen.sh (Windows without Bash: powershell -ExecutionPolicy Bypass -File scripts\docgen.ps1)
just docs-check
```

`docgen` reuses release binaries (building `arw-server`/`arw-cli` in release mode if needed) and refreshes:
- OpenAPI spec (`spec/openapi.yaml`)
- Interface release notes (`docs/reference/interface-release-notes.md`)
- Gating key references (`docs/GATING_KEYS.*`)

Re-run `git status` afterwards to confirm only expected files changed.

## Portable Bundles & Artifacts

Create portable archives for each target architecture you support. For native builds on the host:

```bash
just package          # runs scripts/package.sh
```

To cross-package, invoke `scripts/package.sh --target <triple>` or run through `cargo dist` once the build matrix is green. The script:
- Ensures release blockers are closed.
- Builds `arw-server`, `arw-cli`, and (best effort) `arw-launcher`.
- Emits `dist/arw-<version>-<os>-<arch>.zip` with binaries, docs, and default configs.

If you already built the artifacts, use `--no-build` to package without rebuilding.

## Tagging & Publishing

Use the helper script (it keeps the clippy/test/docgen steps in one place and prepares the tag):

```bash
bash scripts/release.sh vX.Y.Z
```

The script:
1. Runs the clippy/test suite.
2. Regenerates docs/specs.
3. Stages changes, commits `chore(release): vX.Y.Z prep` (no-op if already committed).
4. Creates an annotated tag `vX.Y.Z`.

Review the diff, then push:

```bash
git push origin main --follow-tags
```

CI pipelines pick up the tag and publish:
- Multi-arch container (`ghcr.io/t3hw00t/arw-server:vX.Y.Z`).
- Documentation via GitHub Pages (through `mkdocs` + `mike`).
- Portable bundles if `cargo dist` is configured for automated uploads.

## GitHub Release & Assets

Create or update the GitHub release once CI artifacts are available. The helper script uploads the TypeScript client bundle and keeps notes in sync:

```bash
scripts/gh_release_ts_client.sh --draft vX.Y.Z
```

Attach portable archives from `dist/`, Windows installers, and any supplemental assets (Helm chart tarballs, SBOMs). Copy the changelog summary into the release notes and link to relevant docs (for example, `docs/guide/release.md#portable-bundles--artifacts`).

## Post-Release Verification

- Verify `/healthz`, `/state/runtime_matrix`, and `/state/cluster` on a fresh install.
- Run `arw-cli ping --base http://127.0.0.1:8091` against the released binary.
- Walk through quickstart flows (launcher, CLI, Docker) matching the release notes.
- Confirm docs navigation reflects the new version via `mike list`.
- Notify downstream pack maintainers (Homebrew, winget, etc.) if applicable.

## Related References
- `docs/guide/deployment.md` – install paths and isolation models.
- `docs/guide/docker.md` & `docs/guide/kubernetes.md` – container & Helm guidance.
- `docs/ops/systemd_service.md` – reference systemd units for services.
- `docs/architecture/supply_chain_trust.md` – signing & provenance expectations.
- `docs/developer/status.md` – active tasks and backlog context for release readiness.
