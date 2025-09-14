---
title: Interface Deprecations
---

# Interface Deprecations

_Generated 2025-09-14T03:52:49Z from spec/openapi.yaml. Do not edit._

When an operation is marked deprecated, the runtime emits standard headers (Deprecation, optionally Sunset and Link rel="deprecation").

| Method | Path | Tag | Sunset | Summary |
|---|---|---|---|---|
| GET | `/admin/chat` | Admin/Chat | 2026-01-01T00:00:00Z | Deprecated: Chat history |
| POST | `/admin/chat/clear` | Admin/Chat | 2026-01-01T00:00:00Z | Deprecated: Clear chat history |
| POST | `/admin/chat/send` | Admin/Chat | 2026-01-01T00:00:00Z | Deprecated: Send chat message |
