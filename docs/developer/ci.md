---
title: CI & Releases
---

# CI & Releases

Continuous Integration
- Build and test on Linux and Windows for every push and PR.
- Lint and format checks keep changes tidy.
- Docs are built in strict mode to catch broken links or nav.

Artifacts
- Packaging scripts assemble a portable bundle with binaries and configs.
- Windows and Linux bundles are uploaded as CI artifacts.

Docs site
- MkDocs builds the guide and developer docs.
- GitHub Pages publishes the site from the `gh-pages` branch.

Local helpers
```powershell
# Build & test
scripts/build.ps1
scripts/test.ps1

# Package bundle (release)
scripts/package.ps1
```

Additional checks
```bash
# Unused dependencies
rustup toolchain install nightly --profile minimal
cargo +nightly install cargo-udeps --locked
cargo +nightly udeps --workspace --all-targets

# Verify MSRV
cargo install cargo-msrv --locked
cargo msrv verify
```

