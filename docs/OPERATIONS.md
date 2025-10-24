# ARW Server Operations Guide

Updated: 2025-10-16
Type: Explanation

This document summarizes the operational knobs and endpoints added for stability, crash recovery, and observability.

## Stability + Crash Recovery

- Panic capture: panics are captured and written to `STATE_DIR/crash/` as JSON files (simple payload with message, location, ts_ms).
- Recovery sweep: on boot, the server announces a `service.health` event with `{status:"recovered"}` if crash markers exist, then archives them under `crash/archive/`.
- Safe‑mode: if recent crash markers exist, supervised tasks are deferred for a short period to avoid immediate thrash on boot.

Env vars:
- `ARW_SAFE_MODE_ON_CRASH` (default: `true`) — enable safe‑mode on recent crash.
- `ARW_SAFE_MODE_RECENT_MS` (default: `600000`) — window to consider crash markers recent.
- `ARW_SAFE_MODE_MIN_COUNT` (default: `1`) — min crash markers to trigger safe‑mode.
- `ARW_SAFE_MODE_DEFER_SECS` (default: `30`) — delay before starting supervised tasks.

## Supervised Background Tasks

All critical background loops run under a supervisor. If a loop panics, it is restarted with exponential backoff. If a loop restarts ≥5 times within 30 seconds, the server publishes a degraded `service.health` event with component name and restart counts. Components covered include bus forwarders, read‑models (including `snappy`, `crashlog`, `service_health`), capsule refresh, trust watcher, and research watcher.

## Health + State Endpoints (admin)

- `GET /state/crashlog` — recent crash markers from `STATE_DIR/crash` (and `archive/`).
- `GET /state/service_health` — aggregated `service.health` signals with a small history buffer.
- `GET /state/service_status` — consolidated view: `safe_mode` (active/until), `last_crash`, and `last_health`.

## Metrics (Prometheus)

- `GET /metrics` — Prometheus exposition. New stability signals:
  - `arw_safe_mode_active` (gauge 0/1)
  - `arw_safe_mode_until_ms` (gauge)
  - `arw_last_crash_ms` (gauge)

## HTTP Client Harmonization

All internal HTTP clients now share harmonized defaults (UA, connect timeout, keepalive, idle pool) and use the global request timeout by default.

Env vars:
- `ARW_HTTP_TIMEOUT_SECS` (default: `20`)
- `ARW_HTTP_CONNECT_TIMEOUT_SECS` (default: `3`)
- `ARW_HTTP_TCP_KEEPALIVE_SECS` (default: `60`)
- `ARW_HTTP_POOL_IDLE_SECS` (default: `90`)

## Test Stability + CI

- Test locks are hardened: state‑dir lock is reentrant; ENV lock has a 10s acquisition timeout to avoid deadlocks.
- Nextest configuration (`nextest.toml`) provides per-test and run timeouts; the primary CI job runs `cargo nextest run --profile ci` inside `.github/workflows/ci.yml`.

## Grafana Dashboard

An example Grafana dashboard is provided at `dashboards/grafana/arw-stability.json` with:
- Safe-mode active and minutes since last crash
- Task restarts (window) by task name
- Route p95 latency by path
- Event publish rate and bus lagged events
- Persona signal strength averages by persona and signal label (filter with the Persona/Signal selectors)
- Persona rollups: overall average strength and feedback totals respect the same filters, the trend chart highlights how each persona/signal pair shifts over time, and dedicated lane/slot timelines surface the live retrieval adjustments.

Import it in Grafana (Dashboards → Import) and select your Prometheus datasource.
