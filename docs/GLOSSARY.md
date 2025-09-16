---
title: Glossary
---

# Glossary
Updated: 2025-09-15
Type: Reference

See also: [Configuration](CONFIGURATION.md)

Common terms used throughout the ARW documentation.

Gating
- Centralized authorization for sensitive routes and actions. Uses named keys (e.g., `introspect:*`, `models:*`) and deny contracts. See: Security Hardening (guide/security_hardening.md), Gating Keys (GATING_KEYS.md).

Capsule
- A signed, portable context describing policy, provenance, or execution constraints that can be adopted per request/session. See: Policy (POLICY.md).

Profile
- Runtime tuning preset (e.g., performance, balanced, power‑saver) controlling concurrency, threads, batching, and related hints. See: Quickstart (guide/quickstart.md) and APIs (`/governor/profile`).

Connector / Connection / Link
- Connector: a provider type (HTTP/WS/MCP/local). Connection: a configured instance of a connector. Link: a binding between a connection and a service/route. See: API and Schema (API_AND_SCHEMA.md).

Bus / Event Journal
- In‑process pub/sub for events. Optional on‑disk JSONL journal with replay and filters. Accessible via SSE at `/admin/events` (admin‑gated). See: Admin Endpoints (guide/admin_endpoints.md).

Tool
- A versioned capability with JSON Schemas for input/output/error, exposed across HTTP, events, and MCP. See: API and Schema (API_AND_SCHEMA.md).

CAS (Content‑Addressable Store)
- Storage for immutable artifacts (e.g., models) addressed by content hash; supports atomic swaps and GC.
- Layout for models: `{state_dir}/models/by-hash/<sha256>[.<ext>]` with a per‑ID manifest `{state_dir}/models/<id>.json` describing `file`, `path`, `sha256`, `bytes`, `provider`, and `verified`.
- Optional quota: set `ARW_MODELS_QUOTA_MB` to cap total CAS size (enforced during preflight when enabled). See: Hardware and Models (HARDWARE_AND_MODELS.md).

Downloads Metrics (EWMA)
- A persisted moving average of observed download throughput used to make admission decisions under hard budgets.
- Stored in `{state_dir}/downloads.metrics.json` as `{ ewma_mbps }`.
- Read via `GET /admin/state/models_metrics` for UI/status displays (returns EWMA + counters).

Resume Validators
- Metadata captured from remote responses (`ETag`, `Last-Modified`) and stored alongside partial files.
- Used with `If-Range` to ensure a resumed range request still matches the original content.

Model Daemon
- Centralized model loader/pooler enabling concurrent safe access, leasing, and QoS hints. See: Hardware and Models (HARDWARE_AND_MODELS.md).

RPU (Regulatory Provenance Unit)
- Planned trust component for signature verification and ABAC adoption policy. See: Roadmap (ROADMAP.md).
