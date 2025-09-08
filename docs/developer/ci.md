---
title: CI & Releases
---

# CI & Releases

Continuous Integration
- Build and test on Linux and Windows for every push and PR.
- Lint and format checks keep changes tidy.

Artifacts
- Packaging scripts assemble a portable bundle with binaries and configs.
- Windows and Linux bundles are uploaded as CI artifacts.

Docs site
- Docs are published to GitHub Pages from the `gh-pages` branch.

Local helpers
```powershell
# Build & test
scripts/build.ps1
scripts/test.ps1

# Package bundle (release)
scripts/package.ps1
```

