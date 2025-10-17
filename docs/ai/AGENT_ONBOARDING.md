# Agent Onboarding
Updated: 2025-10-17
Type: Reference

Microsummary: Fast orientation for assistants working in the ARW repo—where to look, what to run, and how to respond safely.

## Start Here
- Read `docs/ai/ASSISTED_DEV_GUIDE.md` for the PLAN → DIFF → tests loop and the lightweight checklist.
- Follow `docs/ai/ai_prompts.md` for safety posture, then cite any harness-provided overrides in your responses.
- Skim `docs/ai/REPO_MAP.md` for the workspace layout before drilling into large surfaces like `README.md`.
- Treat harness or user instructions as the source of truth when they differ from the defaults in these docs. Note the deviation so the next agent has the same context.

## Essential Commands
- Headless bootstrap: `scripts/dev.sh setup-agent` (Bash) or `scripts\dev.ps1 setup-agent` (PowerShell) runs a minimal, non-interactive setup tailored for autonomous agents (headless build, debug profile for `arw-server`, no docs packaging, docgen builds skipped). The helper installs PyYAML via `pip` (setting `PIP_BREAK_SYSTEM_PACKAGES=1` when needed) and skips the `arw-cli` build by default; append `--with-cli` (`-WithCli` on Windows) when your workflow needs that binary.
- Priming toolchains only? Add `--skip-build` / `-SkipBuild` to the setup helpers to install dependencies without compiling; run `cargo build` later once the sandbox is ready.
- Cross-platform helper: `scripts/dev.{sh,ps1}` wraps the common flows (`setup`, `build`, `test`, `verify`, `docs`). Example: `scripts/dev.ps1 verify`.
- Toolchain manager: install [mise](https://mise.jdx.dev) and run `mise install` to provision Rust/Python/Node/jq/rg plus shortcuts like `mise run verify`, `mise run verify:fast`, or `mise run verify:ci`.
- Build: `scripts/build.ps1` (Windows) or `bash scripts/build.sh` (Linux/macOS). Both default to a headless build that skips the Tauri launcher; pass `-WithLauncher` / `--with-launcher` (or set `ARW_BUILD_LAUNCHER=1`) when you specifically need the desktop UI. `make build` / `just build` mirror this headless default, with `make build-launcher` / `just build-launcher` opting into the full workspace build.
- Tests: `scripts/test.ps1` / `bash scripts/test.sh`, or `cargo nextest run` if the helper scripts are unavailable.
- Docs: `mkdocs build --strict` or `just docs-build` (Bash required). On Windows without Bash, pair `mkdocs` with `scripts/docgen.ps1`.
- Docs lint: `python3 scripts/docs_check.py` (set `DOCS_CHECK_FAST=1` or pass `--fast` when you need a lightweight pass that skips mkdocs and deep scans). Shorthand: `mise run docs:check` or `mise run docs:check:fast`.
- Verification guardrail: `scripts/dev.sh verify` (headless default skips the `arw-launcher` crate; pass `--with-launcher` / `-WithLauncher` or set `ARW_VERIFY_INCLUDE_LAUNCHER=1` when you explicitly need the desktop UI, and add `--fast` / `-Fast` to skip doc + UI sweeps). When Node.js is absent the launcher smoke is auto-skipped; export `ARW_VERIFY_REQUIRE_DOCS=1` if you want missing Python/PyYAML to fail the run instead of downgrading to a skip.
- CI parity sweep: `scripts/dev.sh verify --ci` (PowerShell: `scripts\dev.ps1 verify -Ci`) runs the extended guards from GitHub Actions—registry integrity checks, doc generators in `--check` mode, env-guard lint, snappy bench, triad/context/runtime smokes, and legacy surface checks. This mode requires Python 3.11+, Git Bash, and the Rust binaries built locally.
- Runtime smoke safety: before touching the runtime smoke, apply the safe defaults (`just runtime-smoke-safe` or `source scripts/smoke_safe.sh`) so runs stay low-impact (`RUNTIME_SMOKE_SKIP_BUILD=1`, `RUNTIME_SMOKE_USE_RELEASE=1`, `RUNTIME_SMOKE_NICE=1`, `ARW_WORKERS=1/ARW_WORKERS_MAX=1`, GPU policy `skip`). Run `just runtime-smoke-dry-run` to snapshot the plan, then bump `RUNTIME_SMOKE_GPU_POLICY` only with explicit operator approval (and export `RUNTIME_SMOKE_ALLOW_GPU=1`).
- Doc metadata helper: `python scripts/update_doc_metadata.py docs/path/to/page.md` refreshes the `Updated:` stamp (add `--dry-run` to see needed changes without writing).
- Docs bootstrap: `mise run bootstrap:docs` (or `bash scripts/bootstrap_docs.sh`) installs the pinned MkDocs/Material toolchain.
- Offline docs cache: `mise run docs:cache:build` or `scripts/dev.{sh,ps1} docs-cache` produces `dist/docs-wheels.tar.gz`; releases include the same bundle for reuse. Extract it and pass `--wheel-dir` to `bootstrap_docs.sh` when PyPI is unavailable.

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
- Remember that CI layers on registry sync (`scripts/check_feature_integrity.py`, `scripts/check_system_components_integrity.py`) plus triad/runtime smokes; plan those runs when your changes touch the corresponding surfaces.
