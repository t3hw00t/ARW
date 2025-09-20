arw-tauri — Tauri integration glue

Shared helper crate for ARW Tauri apps. Provides:

- A small Tauri plugin exposing commands:
  - `check_service_health(port?) -> bool`
  - `open_debug_ui(port?)`
  - `start_service(port?)`
  - `stop_service(port?)`
- Utility to locate the `arw-server` binary in packaged or dev layouts.

Used by `apps/arw-launcher` and future Tauri companion apps (Debug UI,
Model Manager, Connection Manager).
