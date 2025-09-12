---
title: Configuration
---

# Configuration
{ .topic-trio style="--exp:.7; --complex:.5; --complicated:.3" data-exp=".7" data-complex=".5" data-complicated=".3" }
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
- `ARW_MODELS_MAX_MB`: hard cap for single model download size in MiB (default `4096`).
- `ARW_MODELS_DISK_RESERVE_MB`: reserve free space during downloads in MiB (default `256`).

### Downloads & Budgets
- `ARW_BUDGET_DOWNLOAD_SOFT_MS`: soft budget window in ms (0 = unbounded).
- `ARW_BUDGET_DOWNLOAD_HARD_MS`: hard budget window in ms (0 = unbounded).
- `ARW_BUDGET_SOFT_DEGRADE_PCT`: percentage of soft budget used before a “degraded” status is emitted (default `80`).
- `ARW_DL_MIN_MBPS`: minimum expected throughput used for admission checks when total size is known (default `2.0`).
- `ARW_DL_SEND_RETRIES`: HTTP request retries for initial send before failing (default `2`).
- `ARW_DL_STREAM_RETRIES`: stream read retries (resume with Range) before failing (default `2`).
- `ARW_DL_IDLE_TIMEOUT_SECS`: idle fallback timeout when no hard budget is set (default `300`; set `0` to disable).
- `ARW_DL_EWMA_ALPHA`: smoothing factor for throughput EWMA used in admission decisions (default `0.3`).
- `ARW_DL_NEW`: feature flag for the enhanced downloader path (`1/true/yes` enable; default enabled).

## Hardware Probes & Metrics
- `ARW_ROCM_SMI`: `1` enables ROCm SMI enrichment for AMD GPU metrics on Linux (best‑effort).
- `ARW_DXCORE_NPU`: `1` enables DXCore probe for NPUs on Windows when built with `npu_dxcore` feature.
- `ARW_METRICS_INTERVAL_SECS`: background SSE `Probe.Metrics` interval seconds (default `10`, min `2`).

## CORS & Networking
- `ARW_CORS_ANY`: `1` to relax CORS during development only.

## Launcher & CLI
- `ARW_NO_TRAY`: `1` to skip launching the tray/launcher when starting the service.
- `ARW_HEADLESS`: `1` for headless setup flows in CI.

## Trust & Policy
- `ARW_TRUST_CAPSULES`: path to trusted capsule issuers/keys JSON.

## Tuning Hints
- `ARW_HTTP_TIMEOUT_SECS`: hint for HTTP timeouts used by components that support it.
 - Downloader persists a lightweight throughput EWMA in `{state_dir}/downloads.metrics.json` to improve admission checks across runs.

## Notes
- Sensitive routes include `/admin/*`, `/debug`, `/probe`, `/memory*`, `/models*`, `/governor*`, `/introspect*`, `/chat*`, `/feedback*`.
- Prefer keeping the service bound to `127.0.0.1` or behind a TLS‑terminating reverse proxy.
