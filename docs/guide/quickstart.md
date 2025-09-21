---
title: Quickstart
---

# Quickstart

Updated: 2025-09-21
Type: Tutorial

Run the unified ARW server locally in minutes. The architecture centres on the `/actions` → `/events` → `/state/*` triad; enable `ARW_DEBUG=1` to serve the browser debug panels.

!!! warning "Minimum Secure Setup"
    - Set an admin token: `ARW_ADMIN_TOKEN=your-secret`
    - Keep the service private: bind to `127.0.0.1` or front with TLS
    - Require the header on sensitive calls: `Authorization: Bearer your-secret` or `X-ARW-Admin`
    - Leave `ARW_DEBUG` unset in production

## Prerequisites
- Rust toolchain (`rustup`): https://rustup.rs
- `curl` for quick verification (or `Invoke-WebRequest` on Windows)

## Build and Test

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

## Run the Unified Server (Headless)

The new `arw-server` binary is headless-first. It streams events and state over HTTP/SSE while we finish porting the UI.

=== "Windows"
```powershell
# Headless server (8091 by default)
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -WaitHealth
```

=== "Linux / macOS"
```bash
# Headless server (8091 by default)
bash scripts/start.sh --service-only --wait-health
```

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

Additional views expose models, self descriptions, memory lanes, logic units, orchestrator jobs, and more as they land during the restructure.

## Policy, Leases, and Context

```bash
# Inspect the effective policy
curl -s http://127.0.0.1:8091/state/policy | jq

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

Enable `ARW_DEBUG=1` to expose the debug panels at `/admin/debug` (a local `/debug` alias remains for convenience in dev builds).

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
docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 \
  -e ARW_PORT=8091 \
  -e ARW_ADMIN_TOKEN=dev-admin \
  arw-server:dev
```

`docker compose up` uses the unified server by default.

## Security

- Require `ARW_ADMIN_TOKEN` before invoking `/leases`, `/egress/settings`, or other admin-grade endpoints.
- Use leases to gate outbound HTTP, filesystem writes, or app control (`app.vscode.open`).
- Enable the egress ledger and DNS guard with environment flags (`ARW_EGRESS_LEDGER_ENABLE=1`, `ARW_DNS_GUARD_ENABLE=1`).
- Keep `/events` and `/state/*` behind auth when exposed over the network; they contain action telemetry and internal state.

## Portable Mode

- Set `ARW_STATE_DIR` to relocate state (defaults to `./state`).
- Combine with `ARW_PORTABLE=1` in launchers or scripts to keep all files beside the binaries for USB-style deployment.

## Next Steps

- Read the [Restructure Handbook](../RESTRUCTURE.md) for the canonical roadmap.
- Explore [Context Recipes](context_recipes.md) and [Performance Presets](performance_presets.md) to tune retrieval speed and coverage.
- Run `cargo run -p arw-server` during development for hot reloads and tracing; `ARW_OTEL_EXPORT=stdout` prints spans locally.
