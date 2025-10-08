---
title: Quickstart
---

# Quickstart

Updated: 2025-09-24
Type: Tutorial

Run the unified ARW server locally in minutes. The architecture centres on the `/actions` → `/events` → `/state/*` triad; enable `ARW_DEBUG=1` to serve the browser debug panels.

!!! warning "Minimum Secure Setup"
    - Set an admin token: `ARW_ADMIN_TOKEN=your-secret`
    - Keep the service private: bind to `127.0.0.1` or front with TLS
    - Require the header on sensitive calls: `Authorization: Bearer your-secret` or `X-ARW-Admin`
    - Leave `ARW_DEBUG` unset in production

## Prerequisites
- Rust 1.90+ toolchain (`rustup`): https://rustup.rs
- `curl` for quick verification (or `Invoke-WebRequest` on Windows)

> ARW tracks the latest stable Rust release; run `rustup update` regularly to avoid toolchain drift.

!!! note
    The first launch compiles `arw-server` (and the optional launcher). Expect a multi-minute build on initial setup; subsequent runs reuse the cached binaries.

## Launch Paths

### Option 1 — Portable bundle (fastest)

1. Download the latest release archive from [GitHub Releases](https://github.com/t3hw00t/ARW/releases).
2. Extract it and run the bundled helper:
   - Linux / macOS: `./first-run.sh`
   - Windows: `.\first-run.ps1`
3. The helper generates (or reuses) `state/admin-token.txt`, starts `arw-server`, and prints the Control Room/Debug URLs. Append `--launcher` / `-Launcher` to launch the desktop Control Room when the launcher binary is present, or `--new-token` / `-NewToken` to rotate credentials on demand.

This path skips the Rust toolchain—perfect for quick evaluations or air-gapped installs that prefer prebuilt artifacts.

### Option 2 — Build from source (Rust toolchain)

> The first build compiles `arw-server` and, when requested, the Tauri launcher. Expect a multi-minute compile on cold toolchains; subsequent runs reuse the cached artifacts.

- Linux / macOS  
  `bash scripts/setup.sh --headless`

- Windows  
  `powershell -ExecutionPolicy Bypass -File scripts\setup.ps1 -Headless`

Use `--headless` / `-Headless` to skip the launcher build when WebKitGTK 4.1 + libsoup3 (Linux) or WebView2 (Windows) isn’t available yet. Drop the flag to compile the desktop Control Room once the prerequisites are installed. Add `--minimal` / `-Minimal` to build just the core binaries without packaging or docs, and `--run-tests` / `-RunTests` when you want the workspace test suite as part of setup.

## Build and Test (optional)

Use the standalone build/test helpers when you want finer-grained control or CI-style runs.

=== "Windows"
```powershell
scripts/build.ps1
scripts/test.ps1
```

=== "Linux / macOS"
```bash
bash scripts/build.sh
bash scripts/test.sh
```

The helpers fall back to `cargo test --workspace --locked` when `cargo-nextest` is missing and explain how to install it for faster runs.

## Admin Token Handling

`scripts/start.sh` and `scripts/start.ps1` reuse `state/admin-token.txt` (or generate a new token automatically) and persist it to launcher preferences, so you rarely need to export `ARW_ADMIN_TOKEN` manually. Pass `--admin-token` / `-AdminToken` when you need to supply your own credential, or edit the Control Room → Connection & alerts panel to rotate it later.

## Run the Unified Server

The unified server streams events and state over HTTP/SSE; choose the headless path when you only need the API.

**Headless only** — keep the service running without a desktop UI.

=== "Windows"
```powershell
# Headless server (8091 by default)
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -ServiceOnly -WaitHealth
```

=== "Linux / macOS"
```bash
# Headless server (8091 by default)
bash scripts/start.sh --service-only --wait-health
```

**Control Room + launcher** — start the service and the desktop UI together.

=== "Windows"
```powershell
# Service + launcher (8091 by default)
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -WaitHealth
# Append -InstallWebView2 to auto-install the Evergreen runtime when missing.
```

=== "Linux / macOS"
```bash
# Service + launcher (8091 by default)
bash scripts/start.sh --wait-health
```

!!! tip "Linux launcher requirement"
    The Tauri launcher depends on WebKitGTK 4.1 + libsoup3. Run `bash scripts/install-tauri-deps.sh` on Ubuntu 24.04+, Fedora, or Arch. If your distro lacks those packages (e.g., Ubuntu 22.04, Debian 12 stable), stay headless and open `http://127.0.0.1:8091/admin/ui/control/` or `/admin/debug` in a browser.

The Windows launcher flow prints a summary (service URL, launcher/headless mode, admin token status) and automatically falls back to headless mode if WebView2 is missing, with guidance for installing it.
Open **Launcher Settings** (Control Room → Support) to tweak autostart behaviour, notifications, default port/base, and WebView2 status after the desktop UI loads.

## Verify the Server

```bash
curl -sS http://127.0.0.1:8091/healthz
curl -sS http://127.0.0.1:8091/about | jq
```

You should see metadata that lists the unified endpoints, performance presets, and the current security posture.

## Try an Action End-to-End

Submit a demo action, watch its lifecycle, then fetch it back:

```bash
curl -s -X POST http://127.0.0.1:8091/actions \
  -H 'content-type: application/json' \
  -d '{"kind":"demo.echo","input":{"msg":"hello"}}' | jq

curl -N -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/events?replay=10

curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/actions | jq
```

The events stream shows `actions.submitted`, `actions.running`, and `actions.completed`. When `ARW_ADMIN_TOKEN` is set, `/events` and sensitive `/state/*` views require the token; this is recommended for any non‑local setup.

## Explore State Views

```bash
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/episodes | jq
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/contributions | jq
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/egress/settings | jq
```

Additional views expose models, self descriptions, memory lanes, logic units, orchestrator jobs, and more—capabilities that landed with the restructure and continue to expand with regular releases.

## Policy, Leases, and Context

```bash
# Inspect the effective policy
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/policy | jq

# Create a lease that allows outbound HTTP for 10 minutes
curl -s -X POST http://127.0.0.1:8091/leases \
  -H 'content-type: application/json' \
  -d '{"capability":"net:http","ttl_secs":600}' | jq

# Assemble context (hybrid retrieval with streaming diagnostics)
curl -s -X POST http://127.0.0.1:8091/context/assemble \
  -H 'content-type: application/json' \
  -d '{"q":"demo","lanes":["semantic","procedural"],"limit":12}' | jq
```

These flows emit structured `policy.*`, `working_set.*`, and `leases.*` events. Dashboards can follow along via `/events` or state views.

## Debug UI

Enable `ARW_DEBUG=1` to expose the debug panels at `/admin/debug`.

=== "Windows"
```powershell
scripts/debug.ps1 -Interactive -Open
```

=== "Linux / macOS"
```bash
scripts/debug.sh --interactive --open
```

These helpers prompt for admin tokens, wait for `/healthz`, and launch the UI once ready.

## Desktop Launcher

The Tauri-based launcher targets the unified server. To package or run it:

```bash
just tauri-launcher-build
just tauri-launcher-run
```

## Docker & Compose

A lightweight container image is available for the unified server:

```bash
# Build locally
docker build -f apps/arw-server/Dockerfile -t arw-server:dev .

# Run headless (bind 8091)
export ARW_ADMIN_TOKEN="${ARW_ADMIN_TOKEN:-$(openssl rand -hex 32)}"
docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 \
  -e ARW_PORT=8091 \
  -e ARW_ADMIN_TOKEN="$ARW_ADMIN_TOKEN" \
  arw-server:dev
```

Generate the secret with any equivalent tool if `openssl` is unavailable.

`docker compose up` uses the unified server by default.

## Security

- Require `ARW_ADMIN_TOKEN` before invoking `/leases`, `/egress/settings`, or other admin-grade endpoints.
- Use leases to gate outbound HTTP, filesystem writes, or app control (`app.vscode.open`).
- Enable the egress ledger and DNS guard with environment flags (`ARW_EGRESS_LEDGER_ENABLE=1`, `ARW_DNS_GUARD_ENABLE=1`).
- Keep `/events` and `/state/*` behind auth when exposed over the network; they contain action telemetry and internal state.

## Portable Mode

- Extracted release bundle? Run `./first-run.sh` (Linux/macOS) or `.\first-run.ps1` (Windows) from the archive root to generate/reuse an admin token (`state/admin-token.txt`) and start the unified server on `http://127.0.0.1:8091/`. Add `--launcher` / `-Launcher` to open the Control Room or `--new-token` / `-NewToken` to rotate credentials.
- Set `ARW_STATE_DIR` to relocate state (defaults to `./state`).
- Combine with `ARW_PORTABLE=1` in launchers or scripts to keep all files beside the binaries for USB-style deployment.

## Next Steps

- Read the [Restructure Handbook](../RESTRUCTURE.md) for the canonical roadmap.
- Explore [Context Recipes](context_recipes.md) and [Performance Presets](performance_presets.md) to tune retrieval speed and coverage.
- Run `cargo run -p arw-server` during development for hot reloads and tracing; set `ARW_OTEL=1` (optionally combine with `ARW_OTEL_ENDPOINT=http://collector:4317`) to stream traces to your OTLP collector.
