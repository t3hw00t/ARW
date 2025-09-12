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
```

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
