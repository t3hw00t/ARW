# Agent Onboarding
Updated: 2025-10-09
Type: Reference

Microsummary: Fast orientation for assistants working in the ARW repo—where to look, what to run, and how to respond safely.

## Start Here
- Read `docs/ai/ASSISTED_DEV_GUIDE.md` for the PLAN → DIFF → tests loop and the lightweight checklist.
- Follow `docs/ai/ai_prompts.md` for safety posture, then cite any harness-provided overrides in your responses.
- Skim `docs/ai/REPO_MAP.md` for the workspace layout before drilling into large surfaces like `README.md`.
- Treat harness or user instructions as the source of truth when they differ from the defaults in these docs. Note the deviation so the next agent has the same context.

## Essential Commands
- Build: `scripts/build.ps1` (Windows) or `bash scripts/build.sh` (Linux/macOS). Both default to a headless build that skips the Tauri launcher; pass `-WithLauncher` / `--with-launcher` (or set `ARW_BUILD_LAUNCHER=1`) when you specifically need the desktop UI. `make build` / `just build` mirror this headless default, with `make build-launcher` / `just build-launcher` opting into the full workspace build.
- Tests: `scripts/test.ps1` / `bash scripts/test.sh`, or `cargo nextest run` if the helper scripts are unavailable.
- Docs: `mkdocs build --strict` or `just docs-build` (Bash required). On Windows without Bash, pair `mkdocs` with `scripts/docgen.ps1`.

## Retrieval Tips
- Prefer concise AI reference pages (`docs/ai/*.md`) before loading long-form guides.
- Skip `.arw/tasks.json` unless the task explicitly involves project planning—it is large and changes frequently.
- Link to canonical docs rather than pasting large excerpts; the site is generated from `docs/` via MkDocs.

## Safety & Reporting
- State skipped checks and their rationale, especially when harness policy blocks fmt/clippy/tests.
- Call out security or privacy impact (usually “none”) and flag any deferred follow-up for maintainers.
- Keep diffs focused (< ~300 lines) and avoid opportunistic refactors unless the issue demands a broader change.
