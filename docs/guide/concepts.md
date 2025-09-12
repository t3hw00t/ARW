---
title: Core Concepts
---

# Core Concepts
{ .topic-trio style="--exp:.9; --complex:.5; --complicated:.3" data-exp=".9" data-complex=".5" data-complicated=".3" }

This page orients you to ARW’s moving parts so the rest of the guide makes sense.

Updated: 2025-09-12

## Service
- `arw-svc` is a local HTTP service with a small debug UI. It listens on `127.0.0.1:<port>` by default.

## Tools & Schemas
- Capabilities are exposed as versioned tools with JSON Schemas for inputs/outputs/errors.
- Tools surface via HTTP, events, and MCP. See: API and Schema.

## Event Bus
- Lightweight in‑process bus publishes events (optionally journaling). SSE at `/admin/events` for live streams and replays (admin‑gated).

Unified model
- Treat ARW as a shared object graph (entities + relations) plus a single event stream. UIs (Project Hub, Chat, Training Park, Managers) are lenses on the same truth.

## Connectors, Connections, Links
- Connectors are providers (HTTP/WS/MCP/local). Connections are configured instances. Links bind connections to services/routes.

## Gating & Policy
- Sensitive routes are gated by a central orchestrator. Use `ARW_ADMIN_TOKEN` for admin paths and policy keys to shape ingress/egress.

## Profiles & Governor
- Runtime profiles like performance/balanced/power‑saver adjust concurrency and resource hints. Change via `/governor/profile`.

## State & Portable Mode
- State, cache, and logs live under derived directories. Set `ARW_PORTABLE=1` to keep everything next to the app folder.

## Desktop Launcher (Optional)
- A Tauri app adds a tray, inspector windows (Events, Logs), and quick actions.

## Learn More
- Quickstart: guide/quickstart.md
- Admin Endpoints: guide/admin_endpoints.md
- Security Hardening: guide/security_hardening.md
- API and Schema: API_AND_SCHEMA.md
