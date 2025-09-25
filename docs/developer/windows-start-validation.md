---
title: Windows Start Script Validation
---

# Windows Start Script Validation

This checklist helps validate ARW startup on Windows after changes to `scripts/start.ps1` and the interactive menus.

!!! note "Launcher defaults"
    The interactive launcher workflow boots `arw-server` on port 8091. For headless validation use `scripts/start.ps1 -ServiceOnly`
    or set `ARW_NO_LAUNCHER=1`.

Updated: 2025-09-20
Type: Reference

## Quick pre-reqs
- Install Rust toolchain (rustup): https://rustup.rs
- Optional: build once `powershell -ExecutionPolicy Bypass -File scripts\build.ps1`

## Unified `arw-server` validation (default)

### 1. Launch the start menu
- Run: `powershell -ExecutionPolicy Bypass -File scripts\interactive-start-windows.ps1`.
- Expect the banner to show `Agent Hub (ARW) — Start Menu (Windows)` with the status line `Port=8091 Debug=False Dist=False HealthWait=True/20 s DryRun=False`.
- Confirm the menu lists the options used below (full menu is longer):
  - `1) Configure runtime (port/docs/token)`
  - `3) Start service only`
  - `9) Stop service (/shutdown)`
  - `12) View logs`
  - `13) Save preferences`

### 2. Confirm runtime prompts
- Select `1) Configure runtime (port/docs/token)` and press Enter through the prompts to keep defaults.
- Verify each prompt matches the script:
  - `HTTP port [8091]`
  - `Enable debug endpoints? (y/N)`
  - `Docs URL (optional) []`
  - `Admin token (optional) []`
  - `Use packaged dist/ bundle when present? (y/N)`
  - `Wait for /healthz after start? (Y/n) [Y]`
  - `Health wait timeout secs [20]`

### 3. Start the unified server (headless)
- Pick `3) Start service only`.
- The menu sets `ARW_NO_LAUNCHER=1`, `ARW_PID_FILE=./.arw/run/arw-server.pid`, and `ARW_LOG_FILE=./.arw/logs/arw-server.out.log` before invoking `scripts/start.ps1 -ServiceOnly`.
- Expected CLI output from `start.ps1`:
  - `[start] Launching ... arw-server.exe ... (headless env or unified server)`
  - `[start] Health OK after … → http://127.0.0.1:8091/healthz`
  - A warning about WebView2 is acceptable; the launcher is skipped for the unified server.

### 4. Validate PID/log files and process
- Confirm files exist: `Test-Path .\.arw\run\arw-server.pid` and `Test-Path .\.arw\logs\arw-server.out.log`.
- Inspect the PID and running process:
  - `Get-Content .\.arw\run\arw-server.pid`
  - `Get-Process -Id (Get-Content .\.arw\run\arw-server.pid) | Select-Object ProcessName, Id, Path`
- In the menu, choose `12) View logs → 1) Tail service log (if available)` to ensure it follows `.arw\logs\arw-server.out.log`.

### 5. HTTP smoke tests on port 8091
- `Test-NetConnection -ComputerName 127.0.0.1 -Port 8091` should report `TcpTestSucceeded : True`.
- Use `Invoke-RestMethod` for JSON endpoints (PowerShell 5 requires `-UseBasicParsing`):
  - `Invoke-RestMethod http://127.0.0.1:8091/about`
  - `Invoke-RestMethod http://127.0.0.1:8091/state/actions`
- Stream a short event sample (Ctrl+C after seeing output):
  - `curl --max-time 5 http://127.0.0.1:8091/events?replay=1`
- Expect HTTP 200 responses. `/about` and `/state/actions` return JSON payloads; `/events` should emit at least one NDJSON/SSE line (ensure `ARW_DEBUG=1` or send the admin token when required).

### 6. Shutdown cleanup
- From the menu choose `9) Stop service (/shutdown)`.
- Confirm `Get-Process -Name arw-server` fails and `.\.arw\run\arw-server.pid` no longer matches a running process.

## CLI toggles and packaging checks
- **Hidden window regression**: `powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -ServiceOnly -HideWindow -WaitHealth`. Expect no console window flashes and `[start] Health OK … → http://127.0.0.1:8091/healthz`.
- **Dist bundle**: package with `powershell -ExecutionPolicy Bypass -File scripts\package.ps1`, then run `powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -ServiceOnly -UseDist -WaitHealth`. Confirm it launches `dist\arw-...\bin\arw-server.exe`.
- **NoBuild guard**: temporarily rename `target\release\arw-server.exe`, run `powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -ServiceOnly -NoBuild`, and expect `Service binary not found and -NoBuild specified`.
- **WaitHealth timeout**: `powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -ServiceOnly -WaitHealth -WaitHealthTimeoutSecs 20` should report success when the server is reachable, or warn if it times out.

## Preferences file
- In the menu select `13) Save preferences`.
- Verify `./.arw/env.ps1` contains literal assignments (values reflect your prompts):
  ```powershell
  # ARW env (project-local)
  # dot-source this file to apply preferences
  $env:ARW_PORT = '8091'
  $env:ARW_DOCS_URL = ''
  $env:ARW_ADMIN_TOKEN = ''
  $env:ARW_CONFIG = ''
  $env:ARW_WAIT_HEALTH = '1'
  $env:ARW_WAIT_HEALTH_TIMEOUT_SECS = '20'
  ```
- Re-run the menu to ensure the saved values pre-populate the prompts.

## Notes
- For clean logs between runs, remove `./.arw/logs/*` and `./.arw/run/*`.
