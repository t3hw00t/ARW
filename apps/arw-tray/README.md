Deprecated: use the Tauri-based ARW Launcher

The standalone `arw-tray` binary is superseded by the Tauri-based launcher
(`apps/arw-launcher`). The launcher provides a cross-platform tray, windowed UI,
preferences, and richer integrations. The workspace no longer builds `arw-tray`
by default.

If you still need this legacy tray:
- Build it explicitly with `cargo build -p arw-tray` (not part of workspace).
- Or run the service without a tray using `ARW_NO_TRAY=1`.

