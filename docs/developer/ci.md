---
title: CI & Releases
---

# CI & Releases

Updated: 2025-09-12

## Continuous Integration
- Build and test on Linux and Windows for every push and PR.
- Lint and format checks keep changes tidy.

## Artifacts
- Packaging scripts assemble a portable bundle with binaries and configs.
- Windows and Linux bundles are uploaded as CI artifacts.

## Docs Site
- Docs are published to GitHub Pages from the `gh-pages` branch.

## Local Helpers
```powershell
# Build & test
scripts/build.ps1
scripts/test.ps1

# Package bundle (release)
scripts/package.ps1

# Quick debug run (service with /debug)
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
```
