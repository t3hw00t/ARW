---
title: Desktop Launcher (Tauri)
---

# Desktop Launcher (Tauri)

The ARW Launcher is a Tauri 2 desktop app that provides:

Updated: 2025-09-12

- System tray with Start/Stop Service, Open Debug UI, Events, Logs, Models (stub), Connections (stub).
- In‑app windows:
  - Events: streams `/events` via SSE (Replay 50, optional filter)
  - Logs: polls `/introspect/stats` and shows counters + raw JSON
  - Debug UI opener: browser or embedded window
- Preferences: port, autostart service
- OS login autostart toggle
- Single‑instance, window‑state persistence, notifications on service status changes

## Build & Run

```bash
just tauri-launcher-build
just tauri-launcher-run
```

Linux prerequisites (WebKitGTK 4.1 + libsoup3):

- Debian/Ubuntu: `sudo apt install -y libgtk-3-dev libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev`
- Fedora: `sudo dnf install -y gtk3-devel webkit2gtk4.1-devel libsoup3-devel`
- Arch: `sudo pacman -S --needed gtk3 webkit2gtk-4.1 libsoup3`

Using Nix: `nix develop` (devShell includes required libraries).

## Files & Structure

- App: `apps/arw-launcher/src-tauri`
- Shared glue: `crates/arw-tauri`
- UI: `apps/arw-launcher/src-tauri/ui/` (`index.html`, `events.html`, `logs.html`)
- Icons: `apps/arw-launcher/src-tauri/icons/` (placeholder PNGs; replace with your branding)
  - Regenerate icons following project colors: `./.venv/bin/python scripts/gen_icons.py`

## Notes

- The legacy Rust tray (`apps/arw-tray`) is deprecated and not built by default; the launcher replaces it.
- The launcher reads preferences from the user config dir (e.g., `~/.config/arw/prefs-launcher.json`).
- Windows: on Windows 11, WebView2 Runtime is in-box; on Windows 10/Server you must install the Evergreen Runtime. Use `scripts/webview2.ps1` or Interactive Start → “WebView2 runtime (check/install)”. Server Core lacks desktop features; prefer “Server with Desktop Experience” for UI.
