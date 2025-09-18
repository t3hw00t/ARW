Agent Hub (ARW) Launcher — pre‑alpha

Your private AI control room that can scale and share when you choose. The launcher gives you a friendly tray to start/stop the local service and open the Debug UI.

Minimal Tauri app that provides a system tray with Start/Stop Service and
Open Debug UI actions. This is an initial integration to validate the Tauri
framework and glue code. More UI will be added incrementally.

Build and run (from repo root):

- `just dev-build` (or `cargo build -p arw-launcher`)
- Run `target/debug/arw-launcher` (or `target/release/arw-launcher`)

Notes
- Now prefers the unified server (`arw-server`) by default (port 8091). The legacy bridge (`arw-svc`) remains available as a fallback to power the classic Debug UI.
- Defaults to port 8091 unless `ARW_PORT` is set.
- Uses `crates/arw-tauri` for shared commands and service management.
- For now, no Node/bundler is required; UI is a simple static page served by Tauri.
- Preferences are saved under the user config dir (e.g. `~/.config/arw/prefs-launcher.json`).
- Set `ARW_AUTOSTART=1` to auto-start service on app launch, or enable it in the UI and Save.
- Tray reflects service health and enables/disables Start/Stop accordingly.
- New: OS login autostart toggle (enables launching the app at login).
- New: Open Debug UI inside a window (in addition to default browser).
