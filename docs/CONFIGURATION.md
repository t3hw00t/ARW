---
title: Configuration
---

# Configuration
Updated: 2025-09-12

See also: [Glossary](GLOSSARY.md), [Admin Endpoints](guide/admin_endpoints.md), [Quickstart](guide/quickstart.md)

Centralized reference for ARW environment variables and common flags. Defaults favor local, private, and portable operation.

## Service
- `ARW_PORT`: HTTP listen port (default: `8090`).
- `ARW_PORTABLE`: `1` keeps state/cache/logs near the app bundle.

## Admin & Security
- `ARW_ADMIN_TOKEN`: required token for admin endpoints.
- `ARW_ADMIN_RL`: admin rate limit as `limit/window_secs` (default `60/60`).
- `ARW_DEBUG`: `1` enables local debug mode; do not use in production.

## Docs & Debug UI
- `ARW_DOCS_URL`: URL to your hosted docs for UI links.
- Debug UI is accessible at `/debug` when enabled.

## State & Paths
- `ARW_STATE_DIR`: override state directory.
- `ARW_LOGS_DIR`: override logs directory.

Defaults
- Windows: per-user Known Folders via `directories` (e.g., LocalAppData for data/logs, Roaming for config). No writes to Program Files/HKLM by default.
- Unix: XDG‑compatible locations (e.g., `~/.local/share`, `~/.cache`, `~/.config`).

## Chat & Models
- `ARW_LLAMA_URL`: llama.cpp server endpoint (e.g., `http://127.0.0.1:8080`).
- `ARW_OPENAI_API_KEY`: OpenAI‑compatible API key.
- `ARW_OPENAI_BASE_URL`: custom base URL for OpenAI‑compatible servers.
- `ARW_OPENAI_MODEL`: default model name when using OpenAI‑compatible backend.

## CORS & Networking
- `ARW_CORS_ANY`: `1` to relax CORS during development only.

## Launcher & CLI
- `ARW_NO_TRAY`: `1` to skip launching the tray/launcher when starting the service.
- `ARW_HEADLESS`: `1` for headless setup flows in CI.

## Trust & Policy
- `ARW_TRUST_CAPSULES`: path to trusted capsule issuers/keys JSON.

## Tuning Hints
- `ARW_HTTP_TIMEOUT_SECS`: hint for HTTP timeouts used by components that support it.

## Notes
- Sensitive routes include `/admin/*`, `/debug`, `/probe`, `/memory*`, `/models*`, `/governor*`, `/introspect*`, `/chat*`, `/feedback*`.
- Prefer keeping the service bound to `127.0.0.1` or behind a TLS‑terminating reverse proxy.
