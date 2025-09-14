---
title: Interface Deprecations
---

# Interface Deprecations

_Generated from spec/openapi.yaml (sha256:c38a47eba61b). Do not edit._

When an operation is marked deprecated, the runtime emits standard headers (Deprecation, optionally Sunset and Link rel="deprecation").

| Method | Path | Tag | Sunset | Summary |
|---|---|---|---|---|
| GET | `/admin/chat` | Admin/Chat | 2026-01-01T00:00:00Z | Deprecated: Chat history |
| POST | `/admin/chat/clear` | Admin/Chat | 2026-01-01T00:00:00Z | Deprecated: Clear chat history |
| POST | `/admin/chat/send` | Admin/Chat | 2026-01-01T00:00:00Z | Deprecated: Send chat message |
