---
title: Windows Install & Launcher
---

# Windows Install & Launcher
Updated: 2025-09-20
Type: How‑to

This guide covers running ARW on Windows with the desktop launcher and the headless service. It also notes the current installer status and the path to a signed MSI.

## Requirements

- Windows 10/11 (Desktop Experience). Standard user OK (no admin required).
- Rust 1.90+ toolchain (for developer builds): [rustup](https://rustup.rs)
- Visual Studio Build Tools 2022 with the "Desktop development with C++" workload (for the MSVC linker used by the Rust toolchain): [vs_BuildTools.exe](https://aka.ms/vs/17/release/vs_BuildTools.exe)
- WebView2 Runtime for the Tauri‑based launcher:
  - Windows 11: in‑box
  - Windows 10/Server: install Evergreen Runtime (see below)

> The project follows the latest stable Rust release. Run `rustup update` before major pulls to keep scripts and builds aligned.

## Quickstart (developer path)

```powershell
powershell -ExecutionPolicy Bypass -File scripts\setup.ps1 -Headless
powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -ServiceOnly -WaitHealth
```

- Prefer a leaner install? Append `-Minimal` to `scripts\setup.ps1` to skip doc packaging and keep only the core binaries. Running with both `-Minimal` and `-Headless` stays on the server-only profile; drop `-Headless` once WebView2 is present and you want the desktop Control Room compiled locally.
- Want the Control Room/tray to launch too? Re-run `scripts\start.ps1` without `-ServiceOnly`; the script starts the service and launcher together (WebView2 required).
- Need WebView2? Add `-InstallWebView2` to the start command for a silent Evergreen install when the runtime is missing.
- Need a completely fresh rebuild? Append `-Clean` to `scripts\setup.ps1` to clear existing artifacts before compiling. By default the script reuses incremental build caches for faster reruns.
- Packaging now tolerates missing Git remotes or offline hosts and surfaces a warning instead of failing. Set `ARW_STRICT_RELEASE_GATE=1` (or pass `-StrictReleaseGate`) if you need the setup to stop on open release blockers; CI environments continue to enforce the gate by default.
- When MkDocs is installed automatically, it lands in your user site directory (for example `%LOCALAPPDATA%\Programs\Python\PythonXX\Scripts`). Add that path to your `PATH` if you want to call `mkdocs` directly.
- The start script launches the service in the background and, if present, the desktop launcher.
- Every start run prints a summary (service URL, launcher/headless mode, token status) and falls back to headless mode automatically when WebView2 is missing, with guidance on installing it.
- Tweak defaults (autostart, launch at login, notifications, log paths, WebView2 install) from Control Room → Launcher Settings once the desktop launcher is up.
- `scripts\start.ps1` (and the Linux/macOS variant) reuse `state\admin-token.txt` or generate a token automatically, then persist it to launcher preferences. Pass `-AdminToken` when you need to supply a specific credential; otherwise the script handles it for you.
- If the launcher isn’t built yet, the script attempts a `cargo build -p arw-launcher`.
- If WebView2 is missing, you’ll see a friendly warning and the launcher may prompt to install it.
- Service console: starts minimized by default to dodge antivirus heuristics; use `-HideWindow` to keep it fully hidden like previous versions.
- Want to skip the toolchain entirely? Grab the latest portable `.zip` (and MSI when available) from [GitHub Releases](https://github.com/t3hw00t/ARW/releases), extract, and run `bin\arw-launcher.exe` / `bin\arw-server.exe`.

## Install WebView2 (if needed)

```powershell
powershell -ExecutionPolicy Bypass -File scripts\webview2.ps1
```
Select “Install Evergreen Runtime”. This is required for the launcher on Windows 10/Server.

## Portable bundle (no Rust required)

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package.ps1
powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -ServiceOnly -UseDist -WaitHealth
```
This creates `dist/arw-<version>-windows-<arch>.zip` with:
- `bin/` — `arw-server.exe`, `arw-cli.exe`, optional `arw-launcher.exe`
- `docs/` and configs
- After extracting a release bundle, run `pwsh -ExecutionPolicy Bypass -File .\first-run.ps1` from the archive root to generate/reuse an admin token (`state\admin-token.txt`) and start the unified server headless on `http://127.0.0.1:8091/`. If Windows flags the download, right-click the script -> **Properties** -> **Unblock** or run `Unblock-File .\first-run.ps1` once. Use the optional `-Launcher` switch when you want the Control Room tray app (WebView2 required), or `-NewToken` to rotate credentials on demand.

## Launcher details

- Tray menu: Start/Stop Service, Debug UI (Browser/Window), Windows (Events, Logs, Models, Connections).
- Health: status in tray tooltip with optional desktop notifications.
- Autostart: toggle launcher autostart at login from the UI.
- Connection & alerts now includes a **Test** button that verifies the saved admin token against `/state/projects`, surfacing “valid”, “invalid”, or “offline” states inline before you open the workspaces.
- The hero panel now exposes an **Active connection** selector so you can flip between the local stack and saved remotes without leaving the Control Room. Use the **Manage** shortcut beside it to open the full Connections manager.

## Installer status

- MSI for Rust binaries is configured via cargo-dist and built in CI.
- Launcher MSIs: CI builds Windows MSIs for x64 (primary) and ARM64 (best-effort). Stable filenames on Releases:
  - `arw-launcher-x64.msi`
  - `arw-launcher-arm64.msi` (when built)
  Each MSI includes `arw-server.exe` and `arw-cli.exe` so the launcher can start the service out-of-the-box.
- Current release status (2025-10-03): the latest tagged release (`v0.1.4` at the time of writing) publishes portable `.zip` bundles while the Windows installer gate is tightened. Check [GitHub Releases](https://github.com/t3hw00t/ARW/releases) for the newest tag. Generate an installer locally with `cargo dist build --target x86_64-pc-windows-msvc --installer --no-confirm` (or the ARM64 target) and run `scripts/windows-advanced-gate.ps1` to validate until signed MSIs return to the release page.
- Signing: Code signing is supported in CI when a certificate is provided via secrets; unsigned packages still work but may show SmartScreen prompts. Always record the SHA-256 hash before distribution, for example: `Get-FileHash (Resolve-Path target\dist\*.msi) -Algorithm SHA256`.

## Winget (optional)

- See developer guide for winget manifests: [developer/winget.md](../developer/winget.md).

## Uninstall

- Portable: delete the `dist/arw-*` folder.
- Developer build: remove `target/` outputs; no registry/service entries are created.

## Troubleshooting

- Health: [Local health check](http://127.0.0.1:8091/healthz) (run `curl` while the service is up)
- Logs: `.arw\logs\arw-server.out.log`
- Interactive start menu: `scripts\interactive-start-windows.ps1`
- Quick smoke checks: `arw-cli smoke triad` (action/state/events) and `arw-cli smoke context` (wrappers respect `SMOKE_TRIAD_TIMEOUT_SECS` / `SMOKE_CONTEXT_TIMEOUT_SECS`; both fall back to `SMOKE_TIMEOUT_SECS`, default 600 — set to `0` to stream indefinitely when debugging)
- Need different ports or want to keep the temp directory for debugging? Run `arw-cli smoke --help` to see all flags, or use the wrapper scripts under `scripts\smoke_*.ps1`.
- Running the service on a different machine (Linux server, WSL, container)? Keep this launcher on Windows and point it at the remote via the Active connection picker; the hub/chat/debug browsers remain available at `http://<remote>:8091/admin/...` when you only need a browser.
