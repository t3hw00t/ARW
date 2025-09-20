---
title: Windows Install & Launcher
---

# Windows Install & Launcher
Updated: 2025-09-20
Type: How‑to

This guide covers running ARW on Windows with the desktop launcher and the headless service. It also notes the current installer status and the path to a signed MSI.

Requirements
- Windows 10/11 (Desktop Experience). Standard user OK (no admin required).
- Rust toolchain (for developer builds): https://rustup.rs
- WebView2 Runtime for the Tauri‑based launcher:
  - Windows 11: in‑box
  - Windows 10/Server: install Evergreen Runtime (see below)

Quickstart (developer path)
```powershell
powershell -ExecutionPolicy Bypass -File scripts\setup.ps1
powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -WaitHealth
```
- The start script launches the service in the background and, if present, the desktop launcher.
- If the launcher isn’t built yet, the script attempts a `cargo build -p arw-launcher`.
- If WebView2 is missing, you’ll see a friendly warning and the launcher may prompt to install it.
- Service console: starts minimized by default to dodge antivirus heuristics; use `-HideWindow` to keep it fully hidden like
  previous versions.

Install WebView2 (if needed)
```powershell
powershell -ExecutionPolicy Bypass -File scripts\webview2.ps1
```
Select “Install Evergreen Runtime”. This is required for the launcher on Windows 10/Server.

Portable bundle (no Rust required)
```powershell
powershell -ExecutionPolicy Bypass -File scripts\package.ps1
powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -UseDist -WaitHealth
```
This creates `dist/arw-<version>-windows-<arch>.zip` with:
- `bin/` — `arw-server.exe`, `arw-cli.exe`, optional `arw-launcher.exe`
- `docs/` and configs

Launcher details
- Tray menu: Start/Stop Service, Debug UI (Browser/Window), Windows (Events, Logs, Models, Connections).
- Health: status in tray tooltip with optional desktop notifications.
- Autostart: toggle launcher autostart at login from the UI.

Installer status
- MSI for Rust binaries is configured via cargo‑dist and built in CI.
- Launcher MSIs: CI builds Windows MSIs for x64 (primary) and ARM64 (best‑effort). Stable filenames on Releases:
  - `arw-launcher-x64.msi`
  - `arw-launcher-arm64.msi` (when built)
  Each MSI includes `arw-server.exe` and `arw-cli.exe` so the launcher can start the service out-of-the-box.
- Signing: Code signing is supported in CI when a certificate is provided via secrets; unsigned packages still work but may show SmartScreen prompts.

Winget (optional)
- See developer guide for winget manifests: [developer/winget.md](../developer/winget.md).

Uninstall
- Portable: delete the `dist/arw-*` folder.
- Developer build: remove `target/` outputs; no registry/service entries are created.

Troubleshooting
- Health: `http://127.0.0.1:8091/healthz`
- Logs: `.arw\logs\arw-server.out.log`
- Interactive start menu: `scripts\interactive-start-windows.ps1`
