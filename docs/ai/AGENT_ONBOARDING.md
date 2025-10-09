# Agent Onboarding
Updated: 2025-10-09
Type: Reference

Microsummary: Fast orientation for assistants working in the ARW repo—where to look, what to run, and how to respond safely.

## Start Here
- Read `docs/ai/ASSISTED_DEV_GUIDE.md` for the PLAN → DIFF → tests loop and the lightweight checklist.
- Follow `docs/ai/ai_prompts.md` for safety posture, then cite any harness-provided overrides in your responses.
- Skim `docs/ai/REPO_MAP.md` for the workspace layout before drilling into large surfaces like `README.md`.

## Essential Commands
- Build: `cargo build --workspace` (use `scripts/build.ps1` or `scripts/build.sh` if Bash is unavailable).
- Tests: `cargo nextest run` or `scripts/test.{ps1,sh}` for shell-specific helpers.
- Docs: `just docs-build` (requires Bash) or run `mkdocs build --strict` directly when `just` is inaccessible.

## Retrieval Tips
- Prefer concise AI reference pages (`docs/ai/*.md`) before loading long-form guides.
- Skip `.arw/tasks.json` unless the task explicitly involves project planning—it is large and changes frequently.
- Link to canonical docs rather than pasting large excerpts; the site is generated from `docs/` via MkDocs.

## Safety & Reporting
- State skipped checks and their rationale, especially when harness policy blocks fmt/clippy/tests.
- Call out security or privacy impact (usually “none”) and flag any deferred follow-up for maintainers.
- Keep diffs focused (< ~300 lines) and avoid opportunistic refactors unless the issue demands a broader change.
