Agent Hub (ARW) Launcher
========================

The launcher is the desktop Control Room for Agent Hub: a Tauri 2 application
that orchestrates the local `arw-server`, keeps health and telemetry visible,
and opens the main workspaces (Project Hub, Chat, Training Park, Trial Control,
diagnostics windows, and remote connection tooling).

Highlights
----------

- **Unified service control** – start/stop the bundled `arw-server`, inspect the
  latest health probe, and jump straight into the Debug UI (browser or in-app).
- **Token-aware workspaces** – paste or generate the admin token once, test it
  inline, and unlock Hub, Chat, Training, and other admin surfaces without
  re-entering credentials.
- **Remote connection picker** – store multiple bases + tokens, activate them
  from the hero panel, and open diagnostics windows scoped to the selected base.
- **Live status badges** – SSE connectivity, download progress, and token state
  stay visible in the hero footer; notifications reflect service state changes.
- **Desktop ergonomics** – optional autostart at login, service autostart on
  launch, system tray controls, and quick links to service logs.

Building & Running
------------------

From the repository root:

```bash
# Release build (recommended)
cargo build -p arw-launcher --release

# or use the Just recipe
just dev-build
```

Run the launcher binary once built:

- Debug profile: `target/debug/arw-launcher`
- Release profile: `target/release/arw-launcher`

The launcher expects the unified server binary (`arw-server`) to be available in
the same profile. Use `scripts/start.sh` / `scripts/start.ps1` for a scripted
build + launch across platforms.

Preferences & Tokens
--------------------

- Preferences are stored under the user config directory
  (e.g. `~/.config/arw/prefs-launcher.json` on Linux).
- Use the **Connection & alerts** panel to set the HTTP port, autostart flags,
  desktop notifications, and the admin token.
- The **Test** button calls `/state/projects` with the stored token and surfaces
  “valid”, “invalid”, or “offline” inline before you open any workspaces.
- Generate tokens with the **Generate** button; values are copied to the
  clipboard when possible.

Remote Connections
------------------

- Save remote bases and optional tokens in the Connections manager. Activated
  connections override the local base across launcher windows.
- When a remote base uses plain HTTP, the Control Room surfaces a warning and
  links to the network hardening guide.
- Active downloads, events, models, and logs windows respect the selected base
  and reuse saved credentials.

Platform Notes
--------------

- The launcher targets Linux (WebKitGTK 4.1 + libsoup3), macOS (system WebKit),
  and Windows (WebView2). See `docs/guide/compatibility.md` for package names.
- Preferences can be pre-seeded by writing to the JSON file before first launch.
- `ARW_AUTOSTART=1` forces the service to start when the launcher boots; use the
  UI toggles to persist the behavior.

Troubleshooting
---------------

- Launch with `RUST_LOG=debug` to expand logging.
- Check `launcher-service.log` in the per-user data directory for captured
  stdout/stderr from `arw-server`.
- If WebKit/WebView dependencies are missing, rebuild after installing the
  required packages (`scripts/install-tauri-deps.sh` on Linux, `webview2.ps1`
  on Windows).
