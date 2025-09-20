---
title: Core Concepts
---

# Core Concepts
{ .topic-trio style="--exp:.9; --complex:.5; --complicated:.3" data-exp=".9" data-complex=".5" data-complicated=".3" }

This page orients you to ARW’s moving parts so the rest of the guide makes sense.

Updated: 2025-09-18
Type: Explanation

## Service
- `arw-server` is the unified, headless-first API surface. It binds to `127.0.0.1:8091` by default (override via `ARW_BIND`/`ARW_PORT`) and everything flows through the triad: `POST /actions` submits work, `GET /events` streams progress (resume via `Last-Event-ID`, replay via `?replay=`), and `GET /state/*` serves read-model snapshots. The launcher/debug UI sit on top; enable `ARW_DEBUG=1` when you need the browser panels.

## Tools & Schemas
- Capabilities are exposed as versioned tools (submitted as `/actions` `kind`s) with JSON Schemas for inputs/outputs/errors.
- Tools surface via HTTP, `/events`, and MCP. See: API and Schema.

## Event Bus
- `/events` is the canonical SSE stream. Use `?replay=N` to backfill from the journal and `Last-Event-ID` headers to resume without losing your place. When `ARW_ADMIN_TOKEN` is set, `/events` requires the token.

> Legacy bridge: `/admin/events` remains available for the classic debug UI during transition.

Unified model
- Treat ARW as a shared object graph (entities + relations) plus the `/events` stream and `/state/*` read models. UIs (Project Hub, Chat, Training Park, Managers) are lenses on the same truth.

## Connectors, Connections, Links
- Connectors are providers (HTTP/WS/MCP/local). Connections are configured instances. Links bind connections to services/routes.

## Gating & Policy
- Lease-based ABAC guards sensitive actions. Pick a baseline with `ARW_SECURITY_POSTURE` (`relaxed|standard|strict`), override via `ARW_POLICY_FILE`, and require `ARW_ADMIN_TOKEN` only for privileged calls you explicitly enable.

## Profiles & Governor
- Runtime presets (eco/balanced/performance/turbo) tune concurrency and planner hints. Set `ARW_PERF_PRESET` or adjust knobs like `ARW_HTTP_MAX_CONC`, then inspect `/state/route_stats` to verify the effect.

## State & Portable Mode
- State, cache, and logs live under the `state/` directory. Read-model snapshots publish under `/state/*`. Set `ARW_PORTABLE=1` to keep everything beside the install when you need a self-contained folder.

## Desktop Launcher (Optional)
- A Tauri app layers tray controls and inspectors on top of `arw-server`. While it finishes the port, it can still start the legacy stack for the classic debug panes.

## Learn More
- Quickstart: guide/quickstart.md
- Restructure status: docs/RESTRUCTURE.md
- Security Hardening: guide/security_hardening.md
- API and Schema: API_AND_SCHEMA.md
