---
title: Command Palette
---

# Command Palette
Updated: 2025-09-16
Type: How‑to

The launcher windows (Hub, Chat, Training) ship a lightweight, keyboard‑first command palette.

Open with Ctrl/Cmd‑K.

Built‑in actions
- Open windows: Hub, Chat, Training, Debug, Events
- Open Docs Website
- Refresh models
- SSE replay (reconnect with `?replay=50`)
- Toggle Focus Mode (hide side chrome)
- Toggle Compact Density (smaller spacing; persisted per page)
- Reset UI (Theme/Density/Focus) — returns to OS theme, normal density, and default layout for the current page
- Copy last event JSON (from the single SSE)
- Theme: Auto / Light / Dark — persisted per device; overrides neutrals only
- Capture screen (preview) — calls `ui.screenshot.capture` (requires admin token + `io:screenshot` lease)
- Capture this window (preview) — uses `active_window_bounds` to pass a region to `ui.screenshot.capture`
- Capture region (drag) — overlays a selection box then calls `ui.screenshot.capture` with a `region:x,y,w,h` scope
- Toggle Auto OCR — flips a launcher preference; Chat reflects it and will OCR after capture when enabled

Notes
- The palette is page‑local and does not require admin tokens.
  - Exception: actions that hit admin endpoints (e.g., screenshot) require an admin token in Launcher prefs.
- It is implemented in `apps/arw-launcher/src-tauri/ui/common.js`.
- Additional actions are easy to add for project workflows.
