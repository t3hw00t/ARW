---
title: Interface Deprecations
---

# Interface Deprecations
Updated: 2025-09-15

_Generated from spec/openapi.yaml (sha256:4ee1b181055d). Do not edit._

When an operation is marked deprecated, the runtime emits standard headers (Deprecation, optionally Sunset and Link rel="deprecation").

Deprecated endpoints

- GET /admin/events — prefer GET /events on the unified server
