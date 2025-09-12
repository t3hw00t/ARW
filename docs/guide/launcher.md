---
title: Desktop Launcher (Tauri)
---

# Desktop Launcher (Tauri)

The ARW Launcher is a Tauri 2 desktop app that provides:

Updated: 2025-09-12

- System tray with Start/Stop Service, Open Debug UI, Events, Logs, Models, Connections.
- In‑app windows:
  - Events: streams `/admin/events` via SSE (Replay 50, optional filter)
  - Logs: polls `/introspect/stats` and shows counters + raw JSON
  - Models: list/add/delete/default, start/cancel downloads, progress bars with speed/ETA, optional budget/disk info
  - Connections: add/ping/open Debug UI for multiple local/remote ARW services
  - Debug UI opener: browser or embedded window
- Home card includes a mini downloads widget (Models.* SSE) showing live progress and speed
- Preferences: port, admin token, autostart service, notifications on status changes
- OS login autostart toggle
- Single‑instance, window‑state persistence, notifications on service status changes

## Where Tauri Fits

- Local‑first control plane: a thin, cross‑platform shell that talks to the local ARW service over HTTP + SSE (or WS).
- Unified eventing: use the service’s SSE as the source of truth for episodes/metrics; use Tauri’s event bridge for UI‑level progress/toasts only.
- Security posture: adopt Tauri v2 capabilities/permissions to mirror ARW’s default‑deny policy model (fs/net/mic/cam/gpu/sandbox scopes).
- Plugins for app plumbing: prefer official plugins (SQL, Store, Updater, WebSocket, Single‑Instance, Deep‑Link) to keep footprint small.

Recommended approach
- Keep business logic in the service; keep UI and small local caches in the Tauri app.
- If SSE is unreliable on a platform/proxy, switch to the WebSocket plugin for the event stream.
- Gate all OS integrations (tray, deep links, notifications, file pickers) behind explicit ARW policy prompts with TTL leases; reflect decisions in the sidecar.

Known caveats (and patterns)
- Camera/mic: WebView permission prompts can be sticky on Windows (WebView2). For hard guarantees and auditable leases, capture via a sidecar (ffmpeg/gstreamer) under ARW policy rather than in‑page getUserMedia.
- Security model changes from v1 → v2: adopt capabilities/permissions from day one; avoid the deprecated v1 allowlist idioms.

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
- UI: `apps/arw-launcher/src-tauri/ui/` (`index.html`, `events.html`, `logs.html`, `models.html`, `connections.html`)
  - Models UI: manage inventory and downloads (start/cancel; speed/ETA)
  - Connections UI: saved endpoints, ping, and quick‑open Debug UI
- Icons: `apps/arw-launcher/src-tauri/icons/` (placeholder PNGs; replace with your branding)
  - Regenerate icons following project colors: `./.venv/bin/python scripts/gen_icons.py`

## Notes

- The legacy Rust tray (`apps/arw-tray`) is deprecated and not built by default; the launcher replaces it.
- The launcher reads preferences from the user config dir (e.g., `~/.config/arw/prefs-launcher.json`). Admin token is optional and used for `/admin/*` endpoints.
- Windows: on Windows 11, WebView2 Runtime is in-box; on Windows 10/Server you must install the Evergreen Runtime. Use `scripts/webview2.ps1` or Interactive Start → “WebView2 runtime (check/install)”. Server Core lacks desktop features; prefer “Server with Desktop Experience” for UI.

## Hardening Checklist (Tauri)

- Enforce a strict CSP; never load remote content. Default‑deny (`default-src 'none'`) with explicit `script-src`, `style-src`, `img-src`, `connect-src`, `frame-src`, and `object-src 'none'`.
- Whitelist only the local service origin (host/port) for HTTP.
- Define Tauri v2 capabilities/permissions to expose only required APIs; group them into named sets that mirror ARW policies.
- Ship Single‑Instance and Updater; keep backend updates on their own cadence.
- Persist only small UI caches via Store/SQL; keep authority in the service’s `/state/*` read‑models.
