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

Prerequisites
- Install `cargo-nextest`: `cargo install cargo-nextest`

Commands
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/test.ps1   # or ./scripts/test.sh
scripts/docgen.ps1 # or ./scripts/docgen.sh
just docs-check    # quick docs lint (links/headings), optional

## Stability window
We are currently in a short stability/consolidation phase. Please:
- Favor bug fixes, tests, docs, and internal cleanups over new features
- Keep HTTP/SSE surfaces backward compatible (additive changes only)
- Ensure clippy clean builds (`-D warnings`) for core crates
- Regenerate specs and docs on changes touching APIs/tools

See `docs/developer/stability.md` for the freeze checklist.
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

PR acceptance checklist
- User‑visible docs updated when behavior changes
- Schemas/examples refreshed as needed
- Changelog entry included
- Labels applied (type/*, area/*)
