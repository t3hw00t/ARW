---
title: Tauri API Usage (v2)
---

# Tauri API Usage (v2)

This page documents the Tauri 2 APIs used by the Desktop Launcher and common patterns we follow. Tested with Tauri 2.8.x.

## Menus and Tray
- Submenus: construct with `Submenu::with_id_and_items(...)` or `Submenu::with_items(...)`.
  - Upgrade note: `Submenu::with_id(manager, id, text, enabled, items)` was removed; use `with_id_and_items(manager, id, text, enabled, &[...])`.
- Menu items: create with `MenuItem::with_id(app, "id", "Label", enabled, None::<&str>)`.
- Top-level tray menu: `Menu::with_items(app, &[&svc_sub, &dbg_sub, &windows_sub, &quit])`.
- Tray icon: `TrayIconBuilder::with_id("arw-launcher-tray").tooltip("Agent Hub (ARW)").menu(&menu).on_menu_event(...)`.

Example
```rust
use tauri::menu::{Menu, MenuItem, Submenu};
use tauri::tray::TrayIconBuilder;

let start = MenuItem::with_id(app, "svc-start", "Start Service", true, None::<&str>)?;
let stop  = MenuItem::with_id(app, "svc-stop",  "Stop Service",  true, None::<&str>)?;
let svc   = Submenu::with_id_and_items(app, "svc", "Service", true, &[&start, &stop])?;

let quit  = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
let menu  = Menu::with_items(app, &[&svc, &quit])?;

let _tray = TrayIconBuilder::with_id("arw-launcher-tray")
  .tooltip("Agent Hub (ARW)")
  .menu(&menu)
  .on_menu_event(|app, ev| match ev.id.as_ref() {
    "svc-start" => { /* ... */ }
    "svc-stop"  => { /* ... */ }
    "quit"      => app.exit(0),
    _ => {}
  })
  .build(app);
```

## Windows
- Create the main window with `WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))`.
- Set common attributes: `.title("Agent Hub (ARW) Launcher").inner_size(480.0, 320.0).build()?;`.

## Notifications
- Plugin: `tauri_plugin_notification`.
- Usage: `app.notification().builder().title("...").body("...").show();`

## Autostart & Window State
- Plugins: `tauri_plugin_autostart`, `tauri_plugin_window_state`.
- Configure via `tauri.conf.json` and initialize in the builder chain.

## Capabilities & Permissions (Tauri v2)
- Capability file: `apps/arw-launcher/src-tauri/capabilities/main.json`.
- App permissions: `apps/arw-launcher/src-tauri/permissions/arw.json`.
- When exposing a new command from `crates/arw-tauri` (via `#[tauri::command]`):
  1) Add it to the app permission allowlist.
  2) Ensure the capability references that permission.
  3) Rebuild; unused commands are stripped if `removeUnusedCommands` is enabled.

## Upgrade Notes
- Submenu construction changed in Tauri 2.8.x:
  - Before: `Submenu::with_id(app, "id", "Text", true, menu)`
  - Now: `Submenu::with_id_and_items(app, "id", "Text", true, &[...])`
- Prefer the new helpers to avoid building a `Menu` solely to pass into a submenu.

## References
- Launcher source: `apps/arw-launcher/src-tauri/src/main.rs`
- Shared plugin: `crates/arw-tauri`
- Guide: `docs/guide/launcher.md`
