---
title: Glossary
---

# Glossary
Updated: 2025-09-12

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
- Storage for immutable artifacts (e.g., models) addressed by content hash; supports atomic swaps and GC. See: Hardware and Models (HARDWARE_AND_MODELS.md).

Model Daemon
- Centralized model loader/pooler enabling concurrent safe access, leasing, and QoS hints. See: Hardware and Models (HARDWARE_AND_MODELS.md).

RPU (Regulatory Provenance Unit)
- Planned trust component for signature verification and ABAC adoption policy. See: Roadmap (ROADMAP.md).
