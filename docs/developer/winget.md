---
title: Winget Packaging
---

# Winget Packaging
Updated: 2025-09-15
Type: How‑to

Goal: publish the ARW launcher MSI to the Windows Package Manager (winget).

Overview
- Winget requires a YAML manifest per version with metadata, installer URL, and SHA256.
- Manifests are submitted via PR to the community repo: https://github.com/microsoft/winget-pkgs
- Use the MSI built by `.github/workflows/tauri-windows.yml` on a tagged release (x64; ARM64 when available).
- CI convenience: the workflow uploads `msi-sha256.txt` alongside each MSI artifact (x64 and, if built, ARM64).

Steps
1) Create a GitHub Release tag (e.g., `v0.1.3`). The workflow uploads the MSI under release assets.
2) Download the MSI locally and compute SHA256:
   ```powershell
   $msi = 'Agent Hub (ARW) Launcher_0.1.3_x64_en-US.msi' # adjust name
   (Get-FileHash -Algorithm SHA256 $msi).Hash
   # Or copy from the CI artifact msi-sha256.txt
   ```
3) Generate a manifest with the helper script:
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\winget-gen.ps1 `
     -Version 0.1.3 `
     -InstallerUrl "https://github.com/t3hw00t/ARW/releases/download/v0.1.3/$msi" `
     -InstallerSha256 <SHA256_FROM_STEP_2> `
     -OutDir out-winget
   ```
4) Fork `winget-pkgs` and place the generated YAMLs under the correct path:
   `manifests\a\ARW\Launcher\0.1.3\*.yaml`
5) Submit PR. Address automated checks (naming, URLs, sha). Once merged, `winget install` works.

Notes
- PackageIdentifier used: `ARW.Launcher`
- InstallerType: `msi`
- Scope: `user` (preferred); consider offering `machine` later.
- Keep ProductCode stable across minor updates if practical; Tauri/WiX manages this.
- For ARM64, add a separate installer entry when an ARM64 MSI is published.

Templates
- See `packaging/winget/templates/*` for static templates (single‑file installer with x64+arm64 entries and defaultLocale/version files). You can copy them and replace placeholders or continue using `scripts/winget-gen.ps1` for x64‑only.
