---
title: Interactive Performance Bench
---

# Interactive Performance Bench
Updated: 2025-09-20
Type: How‑to

Status: **Pending rework.** The `snappy_bench` harness previously lived in the legacy
bridge and is being reintroduced for the unified `arw-server`. The commands
below remain for historical reference and will be updated once the new bench
lands.

- Expected target: `apps/arw-server` (bench binary TBD)
- Output: throughput/latency metrics for `/actions` and `/events`

Interim steps:
1. Use the standard `/healthz` and `/about` endpoints to verify the service.
2. Capture request/response timings with external tools (e.g., `hey`, `wrk`)
   against `/actions` and `/events`.
3. Track progress in the [Roadmap → Performance Guardrails](../ROADMAP.md#performance-guardrails).

