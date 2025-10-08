---
title: macOS Install & Launcher
---

# macOS Install & Launcher
Updated: 2025-10-05  
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
bash scripts/setup.sh
bash scripts/start.sh --wait-health --admin-token "$(openssl rand -hex 32)"
```

- `scripts/setup.sh` compiles `arw-server` (headless) and the Tauri launcher. The first build can take several minutes on a cold toolchain.
- `scripts/start.sh` launches the service, waits for `/healthz`, and then opens the Control Room if the launcher is available. Pass `--service-only` to skip the launcher.
- The script exports `ARW_EGRESS_PROXY_ENABLE=1` and `ARW_DNS_GUARD_ENABLE=1` by default. Override them if you need a fully offline profile.

## Control Room (Launcher) highlights

- Open **Connection & alerts** to paste or generate an admin token. The new **Test** button hits `/state/projects` with the saved token and surfaces “valid”, “invalid”, or “offline” states inline.
- Unsaved token edits show a warning badge and explain the next steps (save changes, restart service). When the service is offline the callout tells you to restart before retrying.
- Use **Open Service Log** to jump straight to the current `arw-server` stdout/stderr file in Finder.
- Start/Stop buttons manage the bundled service; status polling every four seconds keeps the badge in sync.

## Portable bundles

```bash
bash scripts/package.sh
```

- Produces `dist/arw-<version>-macos-<arch>.zip` with `arw-server`, `arw-cli`, and the launcher.
- Unzip into a writable location (e.g., `~/Applications/ARW`), then run:

  ```bash
  ./dist/arw-<version>-macos-<arch>/bin/arw-server --help
  ```

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

## Removing ARW

- Stop the service (`scripts/start.sh --service-only --wait-health` followed by `Ctrl+C`).
- Delete the cloned repository or portable bundle.
- Remove state under `~/Library/Application Support/arw` if you want a clean slate.
