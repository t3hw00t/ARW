---
title: Desktop Launcher (Tauri 2)
---

# Desktop Launcher (Tauri 2)
Updated: 2025-10-27
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
- Settings: Configure launcher defaults (autostart, notifications, WebView2, logs)
- Quit: exit the launcher

API reference
- See Developer: [Tauri API](../developer/tauri_api.md) for the exact Tauri APIs we use (submenu/menu patterns, tray, notifications) and upgrade notes.

Status
- Tray tooltip shows “Agent Hub (ARW): online/offline”.
- Start/Stop enable/disable reflects live health checks to `/healthz`.
- Optional desktop notifications on state change; toggle in the Launcher UI.
- Connection & alerts (advanced pane) lets you paste the admin token once; it is stored locally and reused for Projects, Training, Trial, and other admin-only windows.
- The Home view now surfaces inline token health: unsaved edits show a warning badge, the **Test** button probes `/state/projects` with your token, and the status line calls out “valid”, “invalid”, or “offline” states. When the service is unreachable the callout explains how to recover; successful probes hide the warning banner.
- Workspace and diagnostics buttons automatically disable when the service is offline or your admin token is missing, pending save, or invalid. The hint below the buttons explains what to fix so you can re-enable them.
- Admin-only windows raise toast notifications when calls are unauthorized, pointing you back to Connection & alerts so you know where to fix access.
- Home, Models, Conversations, Projects, Training, and Events windows share an SSE status badge (`connecting → connected → retrying`) that announces retry windows, honours server `retry:` hints, auto-refreshes the “last event” timestamp, flags stale streams, and resumes with the last journal id after transient drops (accessible text, `role="status"`, and colour-contrast compliant styling).
- Remote overrides now drive health checks and workspace gating directly. Saved bases (including sub-path deployments like `https://host/arw`) unlock the Home view once the remote `/state/projects` endpoint responds, and fallback to debug-mode (`ARW_DEBUG=1`) keeps surfaces accessible even before you save a token.
- The Home card’s mini downloads row mirrors `models.download.progress` events, including live speed estimates and completion cleanup, without a separate polling loop.
- Home exposes an “Open Service Log” shortcut once the launcher has spawned `arw-server`, so you can jump straight into the current stdout/stderr file without hunting for paths.
- “Copy restart” now falls back to an inline modal if clipboard access is blocked, so users on hardened desktops can still grab the token-aware restart snippet.
- The Logs window includes a Live Output feed that streams launcher-managed stdout/stderr in real time and adds quick copy/open/clear controls for fast triage.

### SSE metrics panel (sidecar)

- In the Metrics lane, a compact SSE block shows:
  - Counters: clients / connections / sent / ~rate/min and errors (parsed from `/metrics`)
  - Sent/min sparkline (sampled)
  - De‑dup miss‑ratio sparkline (hits vs misses)
- Use the “Hide/Show SSE metrics” toggle to collapse/expand the block; the launcher remembers this preference across sessions.
- Tuning tip: if the de‑dup miss‑ratio trend rises under sustained load, consider increasing `ARW_EVENTS_SSE_CAP` on the server.

### Guided setup flow

- The Home view greets you with a guided checklist: **Bring Agent Hub online**, **Secure your workspace**, and **Launch your workspace**. Each block keeps the critical controls in sight and explains what happens next in plain language.
- Live status chips call out whether each step is Ready, In progress, or needs attention, so operators can tell at a glance what remains before launching workspaces.
- Connection & alerts now includes a **Mascot overlay** toggle plus a “Show now” shortcut; Support → Mascot overlay opens the floating companion on demand with the same live hints the checklist surfaces and the live event-stream status (connecting, retrying, stalled).
- “Advanced connection & automation” details stay collapsed until you opt in, keeping port overrides, autostart, notifications, and base selection out of the way for new operators. The launcher still auto-opens the panel when it detects unsaved overrides or a missing token.
- Workspace, diagnostics, and support buttons are grouped by purpose with concise hints so non-technical teammates can see what each window does before launching it. Buttons remain disabled (with inline guidance) until the service is healthy and the token is validated.
- The hero status chip mirrors the latest health probe, while contextual callouts highlight missing tokens, insecure remotes, or desktop-only actions without demanding a separate troubleshooting doc.
- A global “Mode” toggle lets you switch between Guided (default) and Expert views. Expert mode keeps advanced panels open by default and exposes additional diagnostics copy across windows; Guided mode keeps the interface focused on the core three steps.
- Guided mode keeps Projects (formerly Project Hub) lightweight—only the timeline, context, and activity lanes appear by default, while Expert mode restores the full diagnostics sidecar and opens runtime guidance automatically.
- Training runs (formerly Training Park) mirrors the same behaviour: Guided mode focuses on presets, job status, and quick dry-runs, while Expert mode restores cascade telemetry, capsule diagnostics, and the full training sidecar lanes.
- The Events stream honours the same toggle: Guided mode keeps the feed simple with replay controls, while Expert mode re-enables prefix filters, include/exclude text, SLO tuning, and live probe metrics.
- Service Logs follow suit—Guided mode delivers snapshot + tail defaults, and Expert mode unlocks route filters, SLO controls, probe metrics, and CSV exports.
- The Model registry (formerly Model Manager) focuses on the current inventory in Guided mode, while Expert mode exposes concurrency tuning, download tooling, catalogs, hashes, jobs, and egress scopes.
- Experiment Control (formerly Trial Control Center) keeps the gate checklist, status tray, and autonomy controls in Guided mode; Expert mode re-enables approvals, quarantine, feedback lanes, and connections tooling.
- Launcher Settings keeps the general toggles front-and-centre in Guided mode, while Expert mode unlocks base overrides, WebView2 tooling, and log shortcuts.
- Chat keeps the message composer and send shortcuts in Guided mode; Expert mode adds capture tools, reply comparison, and a fully populated sidecar.

## Launcher Settings

- Open from the Support card in the Home view or the tray → Windows → Settings.
- Configure launch behaviour (autostart service, launch at login, desktop notifications) and default port/base overrides shared across windows.
- Inspect WebView2 status on Windows and trigger an Evergreen install/repair directly from the launcher.
- Jump straight to the launcher log directory or the latest service log for fast diagnostics.
- Saved changes broadcast to other launcher windows immediately; headless scripts pick up the same `prefs-launcher.json` values.

## Connections Manager

- Save multiple local or remote server bases plus optional per-connection admin tokens. Bases are normalised (scheme/host/port) so HTTP helpers and SSE reconnects can reuse the credentials reliably.
- The Events, Logs, and Models windows launched from Connections honour the saved base (via the `?base=` override) and reuse the token when present. Status badges distinguish `online`, `auth required`, and `token rejected` responses from `/healthz`.
- Clicking a saved row reloads it into the form for quick edits. Tokens are trimmed client-side and never echoed back in the table; the badge simply signals that a token is stored.
- Remote targets over `http://` or `https://` now work end-to-end from the Home view and window surfaces (SSE, fetch, and tooling). Add an admin token before connecting to anything beyond `127.0.0.1`, and prefer TLS when you forward the service outside a trusted LAN.
- If the active base is remote and still on plain HTTP, the Home view shows a warning callout with a quick link to the network hardening guide. The base badge also turns amber so you can spot unsecured connections at a glance.
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
