# Releasing (Preview → RC → GA)
Updated: 2025-10-27
Type: How‑to

This repo is approaching test‑ready, with path-gated CI smokes across SSE, adapters, OCR, and the Universal Access Kit. Use the manual Build + Deep Checks workflow to validate end‑to‑end behavior before any RC tag.

Readiness checklist

- CI green on main (build-test-docs, mise-guardrails).
- SSE, adapters, OCR smokes all pass (path-gated in CI).
- Universal Access Kit assembles; README.html present; kit zip uploaded as artifact.
- Docs build strict; no broken links.
- Versioning aligned (no `-dev`) for crates to be released.

Go/No‑Go checklist (manual Build Preview)

- Trigger GitHub Actions → Release Preview (manual).
- Confirm artifacts on all OSes:
  - arw-cli and arw-mini-dashboard archives + checksums.
  - Each binary runs `--help` (sanity step is part of the workflow).
- Inspect deep-checks artifacts (Linux):
  - /tmp/economy.json contains an object with `version`.
  - /tmp/route_stats.json parses as JSON.
  - /tmp/events_tail.json has at least one structured event line.
- Universal Access Kit:
  - dist/universal-access-kit.zip exists and unzips.
  - dist/universal-access-kit/README.html present.
  - config/eco-preset.env and docs/* included.

Dry-run artifacts (no tag)

- Trigger GitHub Actions: Build + Deep Checks (Manual).
- Choose targets: default is "all" (Linux/macOS/Windows); select "linux" once the base is stable and you want a faster run.

Cross-OS deep checks

- Linux: starts arw-server (release), validates economy snapshot JSON, route_stats via mini dashboard (once/JSON), and events tail structured output; uploads artifacts and server.log.
- macOS: same validations (uses Python to time-limit events tail); daily brief snapshot is also checked.
- Windows: same validations using PowerShell (ConvertFrom-Json); daily brief snapshot is also checked.

- Outputs per-OS artifacts for `arw-cli` and `arw-mini-dashboard` and a kit zip.
- Sanity-check:
  - Binaries launch with `--help` on each OS.
  - Kit contains `README.html`, `docs/`, and `config/eco-preset.env`.

RC prep

- Bump versions from `0.2.0-dev` to `0.2.0-rc.1` consistently:
  - `[workspace.package].version` in `Cargo.toml`.
  - Affected crate `package.version` fields (`arw-cli`, `arw-mini-dashboard`, etc.).
- Commit and tag: `git tag v0.2.0-rc.1` and push.
- Option A (conservative): continue using Release Preview (manual) and attach artifacts to a draft GitHub Release.
- Option B (after validation): add a tag-driven release workflow.

GA

- Bump to `0.2.0` (remove `-rc.1`), retag `v0.2.0`, publish.
- Optionally enable `cargo-dist` installers in a later phase.

About cargo-dist

- Workspace already defines `workspace.metadata.dist` targets. MSI/PKG installers add platform dependencies; start with archives (zip/tar.gz) only, then graduate to installers.
- A follow-up workflow can run `cargo dist plan/build` to produce signed archives and installers per target.
