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
- Headless bootstrap: `scripts/dev.sh setup-agent` (Bash) or `scripts\dev.ps1 setup-agent` (PowerShell) runs a minimal, non-interactive setup tailored for autonomous agents (headless build, debug profile for arw-server/arw-cli, no docs packaging, docgen builds skipped). The script now ensures PyYAML via `pip` (setting `PIP_BREAK_SYSTEM_PACKAGES=1` when needed) so verification guardrails work out-of-the-box.
- Cross-platform helper: `scripts/dev.{sh,ps1}` wraps the common flows (`setup`, `build`, `test`, `verify`, `docs`). Example: `scripts/dev.ps1 verify`.
- Build: `scripts/build.ps1` (Windows) or `bash scripts/build.sh` (Linux/macOS). Both default to a headless build that skips the Tauri launcher; pass `-WithLauncher` / `--with-launcher` (or set `ARW_BUILD_LAUNCHER=1`) when you specifically need the desktop UI. `make build` / `just build` mirror this headless default, with `make build-launcher` / `just build-launcher` opting into the full workspace build.
- Tests: `scripts/test.ps1` / `bash scripts/test.sh`, or `cargo nextest run` if the helper scripts are unavailable.
- Docs: `mkdocs build --strict` or `just docs-build` (Bash required). On Windows without Bash, pair `mkdocs` with `scripts/docgen.ps1`.
- Docs lint: `bash scripts/docs_check.sh` (set `DOCS_CHECK_FAST=1` or pass `--fast` when you need a lightweight pass that skips mkdocs and deep Python sweeps).

## Tooling Checklist
- Rust toolchain 1.90+ with `cargo`, `rustfmt`, `clippy`, and ideally `cargo-nextest`.
- Python 3.11+ (or newer) on PATH; MkDocs + Material theme (`pip install mkdocs mkdocs-material`) for docs workflows.
- Node.js 18+ (for launcher UI tests such as `apps/arw-launcher/src-tauri/ui/read_store.test.js`).
- Command-line helpers: `jq` and `ripgrep` (`rg`) for scripts and guardrails.
- Optional but helpful: Git Bash on Windows for `bash`-based tooling, `just`, and `make`.

## Retrieval Tips
- Prefer concise AI reference pages (`docs/ai/*.md`) before loading long-form guides.
- Skip `.arw/tasks.json` unless the task explicitly involves project planning—it is large and changes frequently.
- Link to canonical docs rather than pasting large excerpts; the site is generated from `docs/` via MkDocs.

## Safety & Reporting
- State skipped checks and their rationale, especially when harness policy blocks fmt/clippy/tests.
- Call out security or privacy impact (usually “none”) and flag any deferred follow-up for maintainers.
- Keep diffs focused (< ~300 lines) and avoid opportunistic refactors unless the issue demands a broader change.
