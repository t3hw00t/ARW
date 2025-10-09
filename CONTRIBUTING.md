# Contributing to ARW

Thank you for helping shape a calm, capable agent platform.

Principles
- Beauty and harmony: keep UI and code clean and understated.
- Local-first safety: predictable behavior, clear policies.
- Rolling optimizations: make it a little faster and clearer each time.

Workflow
1. Build and test locally.
2. Run format and clippy checks.
3. Update docs and regenerate the workspace status page.
4. Keep commits focused and messages descriptive.
5. Keep interfaces and events consistent with the Feature Matrix and naming policy.
6. During the restructure, keep `docs/RESTRUCTURE.md` up to date for any changes to the triad API (`/actions`, `/events`, `/state`), kernel schemas/behavior, runtime/policy, or migration status.

Prerequisites
- Rust 1.90+ (latest stable via `rustup`)
- Install `cargo-nextest`: `cargo install cargo-nextest`
- Bash-compatible shell for the `just` and `.sh` helpers (Git for Windows, WSL, or another POSIX layer); PowerShell equivalents live alongside each script when you cannot install Bash.
- Node.js 18+ (required for tokens tooling and the `just verify` task; optional if you skip those commands).
- Python 3.11+ with `pip` (used by documentation and spec utilities invoked by `just verify`).
- MkDocs Material: `pip install mkdocs-material` (or run `scripts/docgen.{sh,ps1}` which will prompt if missing).

Build scripts default to a headless profile that skips the Tauri launcher. Pass `--with-launcher` / `-WithLauncher` (or export `ARW_BUILD_LAUNCHER=1`) when you specifically need the desktop UI and have the platform dependencies installed.
The Makefile and Justfile mirror this behavior: `make build` / `just build` run headless by default, while `make build-launcher` or `just build-launcher` opt into compiling the desktop UI.

Commands
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/test.ps1   # or ./scripts/test.sh
scripts/docgen.ps1 # or ./scripts/docgen.sh
just docs-check    # quick docs lint (links/headings), optional
just docs-type-stamp  # insert Type: badges based on path
just docs-stamp       # stamp Updated: dates from git history

# Serve docs locally (hot reload)
just docs-serve

# Feature Matrix (living)
just features-gen  # regenerates docs/reference/feature_matrix.md from interfaces/features.json

# Event kinds linter (dot.case)
python3 scripts/lint_event_kinds.py

> Tip: The `verify` meta-task and docs/tokens scripts require Node, Python, and MkDocs. If you do not have those tools installed, run the individual Rust checks (`fmt`, `clippy`, `cargo nextest`) instead and mention the skipped steps in your notes.

## Rapid iteration guardrails
We track the latest stable Rust toolchain and ship frequently. To keep the project broadly usable while moving fast:
- Keep HTTP/SSE surfaces backward compatible inside a release train; document migrations when breaking changes are unavoidable
- Ship feature flags or toggles with risky changes so rollbacks stay easy
- Ensure clippy stays clean (`-D warnings`) on the current stable toolchain
- Regenerate specs/docs whenever APIs, tools, or schemas change

See `docs/developer/stability.md` for the full move-fast checklist.
```

Rolling optimization checklist
- Hot path review: any obvious allocations, clones, or locks to reduce?
- Async boundaries: spawn wisely, avoid unnecessary blocking.
- Logging: keep context-rich but not noisy; use tracing spans.
- Data shapes: reuse types across API/schema/runtime when possible.
- Build profile: prefer thin LTO; keep codegen-units low for release.

Reasoning quality checklist
- Follow the Performance & Reasoning Playbook (Quick/Balanced/Deep/Verified) for new features.
- Prefer gated self‑consistency and verifier passes over always‑on ensembles.
- Add quality contracts to docs for new output types (claims ↔ sources, metrics, limits).
- Wire changes into the Evaluation Harness with small goldens; avoid regressions.

Docs style
- We follow the Diátaxis model: Tutorials, How‑to, Reference, Explanations. See `docs/developer/docs_style.md`.
- User docs: short, friendly, mildly technical.
- Developer docs: precise, with file paths and commands.
- Use callouts sparingly and let whitespace breathe.
 - Language: Use US English (American). Examples: canceled (not cancelled), color (not colour), disk (not disc).

Docs lint checklist
- Front‑matter `title:` set and a single `#` H1 matching it.
- “Updated: YYYY‑MM‑DD” present near the top when meaningful.
- Headings use Title Case; bullets use sentence case.
- Use OS tabs for multi‑platform commands (“Windows” and “Linux / macOS”).
- Add a short “See also:” block for adjacent pages.
- Prefer relative links within `docs/`; avoid duplicating content between README and docs.
- Link canonical pages: Quickstart, Deployment, Configuration, Admin Endpoints.
- Ensure page is included in `mkdocs.yml` nav.
- Run `just docs-check` and ensure `mkdocs build --strict` passes.
- Use `just docs-type-stamp` and `just docs-stamp` to keep badges and dates consistent.
- Install repo hooks to automate checks: `just hooks-install` (runs format, clippy, tests, spec checks, and docs build when docs change).

PR acceptance checklist
- User‑visible docs updated when behavior changes
- Schemas/examples refreshed as needed
- Changelog entry included
- Labels applied (type/*, area/*)

## Event & Interface Policy

- Event kinds are dot.case only: `foo.bar.baz`. CamelCase is not allowed anywhere (code, docs, assets, or subjects).
- Use the single source of truth for event constants: `crates/arw-topics/src/lib.rs`.
- Publishers must use constants from `topics.rs` — do not inline strings.
- NATS subjects must be dot.case (e.g., `arw.events.task.completed`, `arw.events.node.<node_id>.task.completed`).
- Run `python3 scripts/lint_event_kinds.py` before submitting to ensure no regressions.

## Feature Matrix Policy

- When adding/refactoring a feature, update `interfaces/features.json` with:
  - `name`, `description`, `scope` (surface/audience/layer/maturity)
  - SSoT file paths, HTTP endpoints, read‑models, event topics, env vars
  - `owner` and `deps`
- Regenerate: `just features-gen` and commit `docs/reference/feature_matrix.md`.
- Keep docs in sync with code; avoid drift by updating both in the same PR.
