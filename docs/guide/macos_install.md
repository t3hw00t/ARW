---
title: macOS Install & Launcher
---

# macOS Install & Launcher
Updated: 2025-10-08
Type: How‑to

This guide walks through installing Agent Hub (ARW) on macOS with both the unified service and the desktop launcher. The launcher uses the system WebKit view (no extra runtime required) and now includes inline token verification so you can confirm access before opening the workspaces.

## Requirements

- macOS 13 (Ventura) or newer on Apple Silicon or Intel
- Command Line Tools (`xcode-select --install`)
- Rust 1.90+ toolchain: [rustup](https://rustup.rs)
- Optional: Homebrew for auxiliary tools (`brew install just pkg-config`)

> ARW follows stable Rust releases. Run `rustup update` after pulling to stay aligned with CI builds.

## Quickstart (developer build)

```bash
# Clone the repository, then from the repo root:
bash scripts/setup.sh --headless
bash scripts/start.sh --wait-health
```

- Use `bash scripts/setup.sh --minimal` if you only need the core binaries (`arw-server`, `arw-cli`, `arw-launcher`) and want to skip doc generation and packaging on the first run. Drop `--headless` once you’re ready to build the desktop Control Room locally (WebKit is bundled with macOS).

- Prefer to stay headless? Append `--service-only` to `scripts/start.sh` and open `http://127.0.0.1:8091/admin/ui/control/` in Safari/Chrome instead of launching the desktop Control Room.

- `scripts/setup.sh` compiles `arw-server` (headless) and the Tauri launcher. The first build can take several minutes on a cold toolchain.
- `scripts/start.sh` reuses `state/admin-token.txt` (or generates a token automatically), launches the service, waits for `/healthz`, and then opens the Control Room if the launcher is available. Pass `--service-only` to skip the launcher, or `--admin-token` when you need to supply a specific credential.
- The script exports `ARW_EGRESS_PROXY_ENABLE=1` and `ARW_DNS_GUARD_ENABLE=1` by default. Override them if you need a fully offline profile.
- Prefer to skip compiling? Download the latest macOS portable `.zip` from [GitHub Releases](https://github.com/t3hw00t/ARW/releases), extract, and run `bin/arw-server` / `bin/arw-launcher`.

## Control Room (Launcher) highlights

- Open **Connection & alerts** to paste or generate an admin token. The new **Test** button hits `/state/projects` with the saved token and surfaces “valid”, “invalid”, or “offline” states inline.
- Unsaved token edits show a warning badge and explain the next steps (save changes, restart service). When the service is offline the callout tells you to restart before retrying.
- Use **Open Service Log** to jump straight to the current `arw-server` stdout/stderr file in Finder.
- Start/Stop buttons manage the bundled service; status polling every four seconds keeps the badge in sync.
- Switch between the local stack and saved remotes directly from the **Active connection** selector—no need to leave the hero panel. The adjacent **Manage** button opens the full Connections manager.
- Tweak launcher defaults (autostart service, notifications, default port/base) from **Support → Launcher Settings** once the desktop UI is running.

## Portable bundles

```bash
bash scripts/package.sh
```

- Produces `dist/arw-<version>-macos-<arch>.zip` with `arw-server`, `arw-cli`, and the launcher.
- Unzip into a writable location (e.g., `~/Applications/ARW`), then run:

  ```bash
  ./dist/arw-<version>-macos-<arch>/bin/arw-server --help
  ```
- After extracting a release bundle, run `./first-run.sh` from the archive root to generate/reuse an admin token (`state/admin-token.txt`) and start the unified server headless on `http://127.0.0.1:8091/`. Add `--launcher` to launch the Control Room alongside the service, or `--new-token` when you need a fresh credential.

- Gatekeeper may quarantine unsigned binaries. If you see “cannot be opened because the developer cannot be verified”, remove the quarantine attribute:

  ```bash
  xattr -dr com.apple.quarantine dist/arw-<version>-macos-<arch>
  ```

## Autostart and login items

- Enable “Launch at login” inside the Control Room to register a login item via Tauri. macOS will prompt the first time.
- Toggle “Autostart service” if you want the local `arw-server` to boot automatically when the Control Room launches.

## Troubleshooting

- **Service health** – `curl -sS http://127.0.0.1:8091/healthz`
- **Logs** – `~/Library/Application Support/arw/logs/arw-server.out.log`
- **Launcher rebuild** – `cargo build --release -p arw-launcher`
- **WebKit errors** – ensure the Command Line Tools are up to date; WebKit is bundled with macOS so no extra packages are required.
- **Token rejected** – use the Control Room **Test** button to confirm the value, then restart the service with the new token (`scripts/start.sh --admin-token ...`).
- Running the service somewhere else (Linux box, container, teammate’s machine)? Keep the macOS Control Room and point it at the remote via Active connection, or just open `http://remote-host:8091/admin/debug` in Safari/Chrome when you only need a browser.

## Removing ARW

- Stop the service (`scripts/start.sh --service-only --wait-health` followed by `Ctrl+C`).
- Delete the cloned repository or portable bundle.
- Remove state under `~/Library/Application Support/arw` if you want a clean slate.
