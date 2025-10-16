# Agent Hub (ARW)

<div align="left">

[![CI](https://github.com/t3hw00t/ARW/actions/workflows/ci.yml/badge.svg)](https://github.com/t3hw00t/ARW/actions/workflows/ci.yml)
[![Docs Check](https://github.com/t3hw00t/ARW/actions/workflows/docs-check.yml/badge.svg)](https://github.com/t3hw00t/ARW/actions/workflows/docs-check.yml)
[![Docs](https://img.shields.io/badge/docs-material%20for%20mkdocs-blue)](docs/index.md)
[![Container](https://img.shields.io/badge/ghcr-arw--server-blue?logo=docker)](https://ghcr.io/t3hw00t/arw-server)
[![npm](https://img.shields.io/npm/v/%40arw%2Fclient?label=%40arw%2Fclient)](https://www.npmjs.com/package/@arw/client)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-informational)](#licensing)
[![Status](https://img.shields.io/badge/status-pre--release-purple)](#release-status)
[![Windows x64 Installer](https://img.shields.io/badge/Windows%20x64-Installer-blue?logo=windows)](docs/guide/windows_install.md#installer-status)
[![Windows ARM64 Installer](https://img.shields.io/badge/Windows%20ARM64-Installer-blue?logo=windows)](docs/guide/windows_install.md#installer-status)

</div>

Your private AI control room that can scale and share when you choose.

In plain terms: Agent Hub (ARW) lets you run your own team of AI “helpers” on your computer to research, plan, write, and build—while laying the groundwork for upcoming voice and vision helpers—all under your control. It is local‑first and privacy‑first by default, with the option to securely pool computing power with trusted peers when a project needs more muscle.

> **Restructure update:** `arw-server` is now the sole API surface (headless-first) across every deployment. The old bridge layer and its launch flags have been retired in favour of the unified stack.

Full documentation → [Docs home](docs/index.md)
Assistant quickstart → [Agent Onboarding](docs/ai/AGENT_ONBOARDING.md)

## Quick Links
- [Agent Onboarding](docs/ai/AGENT_ONBOARDING.md) - Workflow primer for assistants and new contributors.
- [Quickstart](docs/guide/quickstart.md) - Run the unified server locally.
- [Runtime Quickstart](docs/guide/runtime_quickstart.md) - Non-technical checklist for preparing managed runtimes (zero-auth mirrors + checksum validation).
- [Repo Map](docs/ai/REPO_MAP.md) - Directory overview for retrieval and navigation.
- [Documentation Home](docs/index.md) - Product overview, guides, and reference docs.

## Release Status
- Legacy release bundles (<= `v0.1.4`) have been retired; no prebuilt downloads are published while we refocus on the next milestone.
- The `main` branch now tracks the `0.2.0-dev` pre-release; GitHub Releases remain frozen until the next milestone cut.
- Prefer source builds (or local packaging scripts) for testing and development during the pre-release cycle.

## Getting Started
- Clone the repo and install Rust 1.90+ via `rustup`.
- Prebuilt installers are currently unavailable; build locally with `scripts/dev.{sh,ps1} setup` or produce a bundle via `scripts/package.{sh,ps1}` if you need a portable archive.
- For source builds, run `scripts/dev.{sh,ps1} setup` (defaults to headless + non-interactive `-Yes`) or follow docs/guide/quickstart.md; pass `--with-launcher` / `-WithLauncher` when desktop UI prerequisites are ready.
- Prefer an all-in-one toolchain bootstrap? Install [mise](https://mise.jdx.dev) and run `mise install` to pin Rust 1.90, Python 3.12, Node.js 18, jq, and ripgrep; `mise run verify`, `mise run verify:fast`, and `mise run verify:ci` wrap the guardrail helpers.

## Build & Test
- Unified helper: `scripts/dev.{sh,ps1}` wraps the common workflow (e.g., `scripts/dev.ps1 build`, `scripts/dev.ps1 verify`).
- Guardrail sweep: `scripts/dev.sh verify` (headless default skips the Tauri crate; pass `--with-launcher` or set `ARW_VERIFY_INCLUDE_LAUNCHER=1` to include it). Use `--fast` to skip docs/UI checks, or `--ci` for a CI‑parity sweep (registry integrity, docgens in `--check` mode, env‑guard, and smokes). Missing Node.js auto‑skips the launcher UI smoke; set `ARW_VERIFY_REQUIRE_DOCS=1` to require Python/PyYAML for doc checks.
- Build: `scripts/build.sh` (Linux/macOS) or `scripts/build.ps1` (Windows). Both default to a headless build that skips the Tauri launcher; add `--with-launcher` / `-WithLauncher` or set `ARW_BUILD_LAUNCHER=1` when you need the desktop UI (requires WebKitGTK 4.1 + libsoup3 on Linux or WebView2 on Windows). `make build` / `just build` follow the same headless default, with `make build-launcher` / `just build-launcher` opting into the full workspace build.
- Format: `cargo fmt --all`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Tests: `cargo nextest run` or `scripts/test.{sh,ps1}`
- Runtime smoke: `just smoke-safe` (apply low-impact defaults) then `just runtime-smoke`. The suite always runs the stub stage, flips CPU/GPU policies to `auto` automatically once you set `RUNTIME_SMOKE_ALLOW_CPU=1` or `RUNTIME_SMOKE_ALLOW_GPU=1`, and will auto-download TinyLlama weights when `HF_TOKEN`/`HUGGINGFACEHUB_API_TOKEN` is exported (disable with `RUNTIME_SMOKE_SKIP_AUTO_WEIGHTS=1`). Provide `LLAMA_SERVER_BIN`/`LLAMA_MODEL_PATH` for real coverage; otherwise the run falls back to simulated markers.
- Docs: `mkdocs build --strict`, `scripts/dev.{sh,ps1} docs`, or `just docs-build` (requires Bash). Windows without Bash: `pwsh -ExecutionPolicy Bypass -File scripts\docgen.ps1` followed by `mkdocs build --strict`. Prefer task wrappers? Use `mise run docs:check` or `mise run docs:check:fast`.
- Consent validation: run `python3 scripts/validate_runtime_consent.py` whenever you edit `configs/runtime/bundles*.json` so audio/vision bundles keep their `metadata.consent` annotations (CI blocks merges otherwise).
- Docs metadata: `python scripts/update_doc_metadata.py docs/<path>.md` refreshes the `Updated:` stamp after edits (add `--dry-run` to preview changes).
- Docs toolchain: `mise run bootstrap:docs` (or `bash scripts/bootstrap_docs.sh`) installs the pinned MkDocs/Material stack. Generate an offline wheel bundle with `mise run docs:cache:build` (archive lands in `dist/docs-wheels.tar.gz`), then reuse it via `mise run bootstrap:docs -- --wheel-dir <path>`.
- Need offline docs? Generate `docs-wheels.tar.gz` locally with `mise run docs:cache:build` (or `scripts/dev.sh docs-cache` / `scripts\dev.ps1 docs-cache`) and reuse it via `scripts/bootstrap_docs.sh --wheel-dir <dir>`.

## Repository Layout
- `crates/` - Core Rust libraries (protocol, kernel, policy, runtime, etc.).
- `apps/` - `arw-server`, CLI, connectors, and launcher surfaces.
- `docs/` - MkDocs site content; published via GitHub Pages.
- `spec/` - JSON Schemas and machine-readable contracts.
- `scripts/` - Cross-platform helpers for setup, testing, and packaging.
- See docs/ai/REPO_MAP.md for more detail and additional directories.

## Architecture & Roadmap
- Unified stack details live in docs/RESTRUCTURE.md (source of truth for the migration).
- Feature status and planned work are tracked in docs/ROADMAP.md and docs/reference/feature_matrix.md.
- Release notes land in CHANGELOG.md and `RELEASE_NOTES_*` files.

## Contributing
- Review CONTRIBUTING.md, CODE_OF_CONDUCT.md, and the Agent Onboarding guide before opening changes.
- Follow the assisted development workflow (PLAN -> DIFF -> tests) outlined in docs/ai/ASSISTED_DEV_GUIDE.md.

## Licensing
- Dual licensed under [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE).
- Security disclosures: see SECURITY.md.

## Community & Support
- Issues and discussions live on GitHub.
- Automation dashboards and task tracking sit under .arw/ (large generated files are excluded from releases).
- For status badges, CI pipelines, and deployment artifacts, see the badges at the top of this README.
