---
title: Orchestrator Validation
---

# Orchestrator Validation
Updated: 2025-10-09
Type: How-to

## Local Queue
- Covers priority ordering, lease expiry, and retry notifications.
- Run: `cargo test -p arw-core` (runs both unit and integration suites).

## NATS Queue (Manual)
- Prereq: running `nats-server` exposing `nats://127.0.0.1:4222` (for example `docker run --rm -p 4222:4222 nats:2.10.19`).
- Enable crate feature and run ignored test:
  - `cargo test -p arw-core --features nats --test nats_queue_flow -- --ignored`
- Verifies round-trip enqueue/dequeue through core queue groups.
