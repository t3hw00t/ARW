---
title: Windows Start Script Validation
---

# Windows Start Script Validation

This checklist helps validate ARW startup on Windows after changes to `scripts/start.ps1` and the interactive menus.

Updated: 2025-09-13
Type: Reference

Quick pre-reqs
- Install Rust toolchain (rustup): https://rustup.rs
- Optional: build once `powershell -ExecutionPolicy Bypass -File scripts\build.ps1`

Service + launcher (default)
- Run: `powershell -ExecutionPolicy Bypass -File scripts\interactive-start-windows.ps1`
- Pick “Start launcher + service”.
- Expect: no extra console window for the service (runs hidden), launcher appears with a system tray icon.
- Check `.arw\run\arw-svc.pid` and `.arw\logs\arw-svc.out.log` exist.
- Open: `http://127.0.0.1:8090/debug` and `.../spec`.
  - Tip: The Start menu lets you toggle health wait (and timeout) under “Configure runtime”.

Service only (CLI)
- Set `ARW_NO_LAUNCHER=1` from the menu (or via environment) and start “service only”.
- Expect: service starts in background; PID/log file present when configured.

Dist bundle
- Package: `powershell -ExecutionPolicy Bypass -File scripts\package.ps1`
- Start with bundle: `powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -UseDist -WaitHealth`
- Expect: service from `dist\arw-...\bin\arw-svc.exe` and health check completes.

NoBuild behavior
- Remove or rename `target\release\arw-svc.exe`.
- Run: `powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -NoBuild`
- Expect: script errors out early with “Service binary not found and -NoBuild specified”.

Health check
- Use `-WaitHealth -WaitHealthTimeoutSecs 20` to have the script poll `http://127.0.0.1:<port>/healthz` after starting in background.
- Expect: info message “Health OK …” on success; warning if not ready within timeout.

Preferences file
- From the start menu, “Save preferences”.
- Verify `./.arw/env.ps1` contains literal lines like `$env:ARW_PORT = '8090'` (not expanded values).

Notes
- The launcher is optional; use CLI-only mode or `ARW_NO_LAUNCHER=1` (alias: `ARW_NO_TRAY=1`) to skip it.
- For clean logs, delete `./.arw/logs/*` between runs.
