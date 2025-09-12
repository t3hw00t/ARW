---
title: Windows Compatibility Gate
---

# Windows Compatibility Gate

Updated: 2025-09-12

Use this checklist to gate PRs/releases for Windows.

Core standards
- Known Folders: per-user data in LocalAppData/Roaming; avoid Program Files/HKLM writes. ARW defaults use platform directories via `directories` (config/cache/data local). Override with `ARW_STATE_DIR`, `ARW_LOGS_DIR` as needed.
- Server Core: validate separately; UI features (tray/WebView2) require “Desktop Experience”. Document minimum OS.

Runtime dependencies (Tauri/WebView2)
- WebView2 Evergreen Runtime present check and bootstrapper: use `scripts/webview2.ps1` or Interactive Start → “WebView2 runtime (check/install)”. On Windows 11 it’s in‑box; Windows 10/Server requires Evergreen.
- Tauri bundling: `tauri.conf.json` includes Windows `webviewInstallMode: "downloadBootstrapper"`.

Auto-audit tools (optional to strict)
- WACK (Windows App Certification Kit): validate desktop/MSIX packages. Run locally or in a gated self‑hosted agent. Store report.
- MSIX Packaging/Validation + PSF: if adopting MSIX, validate and use shims for runtime quirks.
- MSI ICE validation (if shipping MSI via cargo‑dist): run with Orca/MsiVal2 and keep the report.
- SignTool verification: ensure all EXEs/MSIs are Authenticode‑signed and verify clean to minimize SmartScreen friction.
- Application Verifier: run basic heap/handle/LUA checks against `arw-svc.exe` (and UI EXEs).
- SUA + Compatibility Administrator (ADK): scan for UAC write/registry issues; apply shims if needed.

Sysinternals (runtime drift)
- AccessChk: audit effective ACLs for files/registry/services touched (prove standard‑user operation).
- ProcMon/ListDLLs: isolate environment‑specific failures (file/registry/load‑order).
- Sigcheck: inventory signatures and reputation of loaded modules (optional).

PowerShell linting
- Run PSScriptAnalyzer over `scripts/` with compatibility rules targeting Windows PowerShell 5.1 and PowerShell 7.x.

CI matrix (Windows)
- GitHub Actions matrix: `windows-2022` and `windows-latest` (migrates to 2025). Gate on: build+tests → PSScriptAnalyzer (informational) → optional AppVerifier/WACK/MSI ICE in hardened pipelines.

ARW specifics
- MSI quality (if used): validate ICE rules and signature in release checks; README should reference signed installers only.
- Launcher/WebView2: presence check during setup; document minimum OS and Desktop Experience expectation.
- Standard‑user posture: prefer per‑user paths; no writes to Program Files/HKLM; optional AccessChk in smoke.

Quick commands
- WebView2: `powershell -ExecutionPolicy Bypass -File scripts\webview2.ps1` → `WebView2-Menu`
- Start (service only): `powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -WaitHealth`

Automation
- Script: `scripts/windows-advanced-gate.ps1` runs SignTool verify, attempts MSI ICE (if tools installed), and surfaces AppVerifier/WACK hints. Set `ARW_STRICT_SIGN_VERIFY=1` to fail unsigned artifacts.
- Workflow: `windows-advanced-gate` (manual or on release). Use the workflow_dispatch input `strict_sign=true` to enforce signing.

