---
title: Quickstart
---

# Quickstart

Updated: 2025-09-18
Type: Tutorial

Run the unified ARW server locally in minutes. The new architecture focuses on the `/actions` → `/events` → `/state/*` triad; the legacy `arw-svc` with the classic debug UI remains available behind a `--legacy` flag while the restructure continues.

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

*Need the legacy debug UI?* Pass `-Legacy` (Windows) or `--legacy` (Linux/macOS) to start `arw-svc` instead. See [Legacy UI Bridge](#legacy-ui-bridge) below.

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

curl -N http://127.0.0.1:8091/events?replay=10

curl -s http://127.0.0.1:8091/state/actions | jq
```

The events stream shows `actions.submitted`, `actions.running`, and `actions.completed`. Any client can subscribe to `/events` (optionally with `?prefix=` and `?replay=` filters) to stay in lock-step with the server.

## Explore State Views

```bash
curl -s http://127.0.0.1:8091/state/episodes | jq
curl -s http://127.0.0.1:8091/state/contributions | jq
curl -s http://127.0.0.1:8091/state/egress/settings | jq
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

## Legacy UI Bridge

The classic service remains available while we finish porting surfaces to the unified stack.

=== "Windows"
```powershell
powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -Legacy -WaitHealth
```

=== "Linux / macOS"
```bash
bash scripts/start.sh --legacy --wait-health
```

Legacy mode listens on port `8090`, serves the debug UI, and powers existing launcher workflows. Use it when you need the full GUI today, but prefer the unified server for API-driven integrations, automation, and future features.

## Desktop Launcher (Legacy Bridge)

The Tauri-based launcher currently targets the legacy service bundle. To package or run it:

```bash
just tauri-launcher-build
just tauri-launcher-run -- --legacy
```

On Windows:
```powershell
scripts/interactive-start-windows.ps1  # prompts for legacy vs. unified
```

The launcher will be updated to speak the unified API once the new UI lands. Until then it auto-starts `arw-svc` when invoked without overrides.

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

`docker compose up` now uses the unified server by default. Set `LEGACY=1` and swap in `apps/arw-svc/Dockerfile` if you must run the legacy stack.

## Security

- Require `ARW_ADMIN_TOKEN` before invoking `/leases`, `/egress/settings`, or other admin-grade endpoints.
- Use leases to gate outbound HTTP, filesystem writes, or app control (`app.vscode.open`).
- Enable the egress ledger and DNS guard with environment flags (`ARW_EGRESS_LEDGER_ENABLE=1`, `ARW_DNS_GUARD_ENABLE=1`).
- Keep `/events` behind auth when exposed over the network; it contains action telemetry.

## Portable Mode

- Set `ARW_STATE_DIR` to relocate state (defaults to `./state`).
- Combine with `ARW_PORTABLE=1` in launchers or scripts to keep all files beside the binaries for USB-style deployment.

## Next Steps

- Read the [Restructure Handbook](../RESTRUCTURE.md) for the canonical roadmap.
- Explore [Context Recipes](context_recipes.md) and [Performance Presets](performance_presets.md) to tune retrieval speed and coverage.
- Run `cargo run -p arw-server` during development for hot reloads and tracing; `ARW_OTEL_EXPORT=stdout` prints spans locally.
