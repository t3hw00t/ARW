---
title: Desktop Launcher (Tauri 2)
---

# Desktop Launcher (Tauri 2)

The tray-based launcher lives at `apps/arw-launcher/src-tauri`. It uses Tauri 2 with the capabilities + permissions model.

Build locally
```bash
cargo build -p arw-launcher
```

Capabilities and Permissions
- Capability: `apps/arw-launcher/src-tauri/capabilities/main.json`
  - Grants defaults for core plugins and plugin window state, autostart, and notifications.
  - References an app-defined permission `arw-commands`.
- App permissions: `apps/arw-launcher/src-tauri/permissions/arw.json`
  - Explicit allowlist of custom commands exposed by the `arw-tauri` plugin.

When adding new commands to the plugin:
1) Export the command with `#[tauri::command]` in `crates/arw-tauri`.
2) Add the command name to `permissions/arw.json` under `commands.allow`.
3) Rebuild. With `build.removeUnusedCommands: true` in `tauri.conf.json`, non-allowed commands are stripped.

Windows
- The `tauri.conf.json` sets `bundle.windows.webviewInstallMode: downloadBootstrapper` for WebView2.

