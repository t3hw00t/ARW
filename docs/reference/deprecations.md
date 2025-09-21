---
title: Interface Deprecations
---

# Interface Deprecations
Updated: 2025-09-21
Type: Reference

_Generated from spec/openapi.yaml (sha256:131c6d3e0a7e). Do not edit._

When an operation is marked deprecated, the runtime emits standard headers (Deprecation, optionally Sunset and Link rel="deprecation").

| Method | Path | Tag | Sunset | Summary |
|---|---|---|---|---|
| GET | `/admin/introspect/stats` | Admin/Introspect |  |  |
| POST | `/admin/projects/create` | Admin/Projects |  |  |
| GET | `/admin/projects/file` | Admin/Projects |  |  |
| POST | `/admin/projects/file` | Admin/Projects |  |  |
| POST | `/admin/projects/import` | Admin/Projects |  |  |
| GET | `/admin/projects/list` | Admin/Projects |  |  |
| GET | `/admin/projects/notes` | Admin/Projects |  |  |
| POST | `/admin/projects/notes` | Admin/Projects |  |  |
| POST | `/admin/projects/patch` | Admin/Projects |  |  |
| GET | `/admin/projects/tree` | Admin/Projects |  |  |
| GET | `/admin/state/actions` | Admin/State |  | Recent actions stream (rolling window). |
| GET | `/admin/state/beliefs` | Admin/State |  | Current beliefs snapshot derived from events. |
| GET | `/admin/state/cluster` | Admin/State |  | Cluster nodes snapshot (admin-only). |
| GET | `/admin/state/intents` | Admin/State |  | Recent intents stream (rolling window). |
| GET | `/admin/state/observations` | Admin/State |  | Recent observations from the event bus. |
| GET | `/admin/state/route_stats` | Admin/State |  | Legacy admin alias for route stats. |
| GET | `/admin/state/world` | Admin/State |  | Project world model snapshot (belief graph view). |
| GET | `/admin/state/world/select` | Admin/State |  | Select top-k claims for a query. |
