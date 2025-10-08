---
title: Desktop Launcher (Tauri 2)
---

# Desktop Launcher (Tauri 2)
Updated: 2025-10-05
Type: How‑to

The tray-based launcher lives at [apps/arw-launcher/src-tauri](https://github.com/t3hw00t/ARW/blob/main/apps/arw-launcher/src-tauri). It uses Tauri 2 with the capabilities + permissions model and exclusively targets the unified `arw-server` binary that now provides the full API surface.

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
- Connection & alerts (advanced pane) lets you paste the admin token once; it is stored locally and reused for Projects, Training, Trial, and other admin-only windows.
- The Control Room now surfaces inline token health: unsaved edits show a warning badge, the new **Test** button probes `/state/projects` with your token, and the status line calls out “valid”, “invalid”, or “offline” states. When the service is unreachable the callout explains how to recover; successful probes hide the warning banner.
- Workspace and diagnostics buttons automatically disable when the service is offline or your admin token is missing, pending save, or invalid. The hint below the buttons explains what to fix so you can re-enable them.
- Admin-only windows raise toast notifications when calls are unauthorized, pointing you back to Connection & alerts so you know where to fix access.
- Home, Models, Chat, Hub, Training, and Events windows share an SSE status badge (`connecting → connected → retrying`) that announces retry windows, honours server `retry:` hints, auto-refreshes the “last event” timestamp, flags stale streams, and resumes with the last journal id after transient drops (accessible text, `role="status"`, and colour-contrast compliant styling).
- The home card’s mini downloads row mirrors `models.download.progress` events, including live speed estimates and completion cleanup, without a separate polling loop.
- Control Room exposes an “Open Service Log” shortcut once the launcher has spawned `arw-server`, so you can jump straight into the current stdout/stderr file without hunting for paths.
- The Logs window includes a Live Output feed that streams launcher-managed stdout/stderr in real time and adds quick copy/open/clear controls for fast triage.

## Connections Manager

- Save multiple local or remote server bases plus optional per-connection admin tokens. Bases are normalised (scheme/host/port) so HTTP helpers and SSE reconnects can reuse the credentials reliably.
- The Events, Logs, and Models windows launched from Connections honour the saved base (via the `?base=` override) and reuse the token when present. Status badges distinguish `online`, `auth required`, and `token rejected` responses from `/healthz`.
- Clicking a saved row reloads it into the form for quick edits. Tokens are trimmed client-side and never echoed back in the table; the badge simply signals that a token is stored.
- Remote targets over `http://` or `https://` now work end-to-end from the Control Room and window surfaces (SSE, fetch, and tooling). Add an admin token before connecting to anything beyond `127.0.0.1`, and prefer TLS when you forward the service outside a trusted LAN.
- If the active base is remote and still on plain HTTP, the Control Room shows a warning callout with a quick link to the network hardening guide. The base badge also turns amber so you can spot unsecured connections at a glance.
- Rows poll every 10 seconds; the in-page SSE indicator follows the same base so metrics stay scoped to the selected connection.
- Every launcher window now renders a base badge and disables the local port field whenever a saved override is active, making it explicit which host/port is being queried (and preventing accidental port mismatches).
- Activate any saved connection to make it the global override (stored locally); a quick “Deactivate” control and badge highlight make it easy to flip between local and remote targets.

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
- Activity lane thumbnails and the Screenshots Gallery subscribe to `screenshots.captured`; recent captures gain quick actions (Open, Copy MD, Save to project, Annotate). Save to project now pipes through `project.notes.append` when “Append to notes” is enabled, so copied assets are linked in `NOTES.md` automatically. OCR completions arrive on `screenshots.ocr.completed`, automatically refreshing alt text and cached Markdown snippets.
- Requires leases: capture/annotate prompt for `io:screenshot`, OCR additionally needs `io:ocr`.

Design & UI
- Launcher pages include `tokens.css` (design tokens) and `ui-kit.css` (primitives) for consistent visuals.
- See Developer → Design Theme and Developer → UI Kit for details.
