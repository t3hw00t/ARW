---
title: Desktop Launcher (Tauri 2)
---

# Desktop Launcher (Tauri 2)
Updated: 2025-09-20
Type: How‑to

The tray-based launcher lives at [apps/arw-launcher/src-tauri](https://github.com/t3hw00t/ARW/blob/main/apps/arw-launcher/src-tauri). It uses Tauri 2 with the capabilities + permissions model and exclusively targets the unified `arw-server` binary (the legacy `arw-svc` bridge is no longer supported).

Launch
```bash
# Dev run (hot)
cargo run -p arw-launcher

# Scripted run (launcher + unified server)
bash scripts/start.sh --wait-health

# Headless (service only)
bash scripts/start.sh --service-only --wait-health
```

Linux dependencies (Tauri)
- Install WebKitGTK 4.1 + libsoup3 dev packages: `bash scripts/install-tauri-deps.sh`
- Or enter the Nix dev shell: `nix develop`

Menu
- Service: Start Service, Stop Service
- Debug: Open Debug (Browser), Open Debug (Window)
- Windows: Events, Logs, Models, Connections
- Quit: exit the launcher

API reference
- See Developer: [Tauri API](../developer/tauri_api.md) for the exact Tauri APIs we use (submenu/menu patterns, tray, notifications) and upgrade notes.

Status
- Tray tooltip shows “Agent Hub (ARW): online/offline”.
- Start/Stop enable/disable reflects live health checks to `/healthz`.
- Optional desktop notifications on state change; toggle in the Launcher UI.

Build locally
```bash
cargo build -p arw-launcher
```

Capabilities and Permissions
- Capability: [apps/arw-launcher/src-tauri/capabilities/main.json](https://github.com/t3hw00t/ARW/blob/main/apps/arw-launcher/src-tauri/capabilities/main.json)
  - Grants defaults for core plugins and plugin window state, autostart, and notifications.
  - References an app-defined permission `arw-commands`.
- App permissions: [apps/arw-launcher/src-tauri/permissions/arw.json](https://github.com/t3hw00t/ARW/blob/main/apps/arw-launcher/src-tauri/permissions/arw.json)
  - Explicit allowlist of custom commands exposed by the `arw-tauri` plugin.

When adding new commands to the plugin:
1) Export the command with `#[tauri::command]` in [crates/arw-tauri](https://github.com/t3hw00t/ARW/blob/main/crates/arw-tauri).
2) Add the command name to `permissions/arw.json` under `commands.allow`.
3) Rebuild. With `build.removeUnusedCommands: true` in `tauri.conf.json`, non-allowed commands are stripped.

Windows
- The `tauri.conf.json` sets `bundle.windows.webviewInstallMode: downloadBootstrapper` for WebView2.
 - Quickstart and WebView2 install: see [Windows Install](windows_install.md).

Screenshots & Gallery
- Command palette exposes “Capture screen/window/region” actions that invoke `ui.screenshot.capture` with preview downscaling; annotate and OCR follow-ups surface automatically when the tool returns `preview_b64`.
- Chat toolbar mirrors the palette with Capture, Capture window, and Capture region buttons plus inline Annotate/OCR toggles so agents can share their current view on request.
- Activity lane thumbnails and the Screenshots Gallery subscribe to `screenshots.captured`; recent captures gain quick actions (Open, Copy MD, Save to project, Annotate).
- Requires leases: capture/annotate prompt for `io:screenshot`, OCR additionally needs `io:ocr`.

Design & UI
- Launcher pages include `tokens.css` (design tokens) and `ui-kit.css` (primitives) for consistent visuals.
- See Developer → Design Theme and Developer → UI Kit for details.
