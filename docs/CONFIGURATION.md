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
- `ARW_MODELS_MAX_CONC`: max concurrent model downloads (default `2`; `0` or `<1` treated as `1`).
- `ARW_MODELS_QUOTA_MB`: optional total on‑disk quota for all models stored in CAS (sum of `state/models/by-hash/*`) in MiB. When set, downloads are denied if projected total would exceed the quota.

### Downloads & Budgets
- `ARW_BUDGET_DOWNLOAD_SOFT_MS`: soft budget window in ms (0 = unbounded).
- `ARW_BUDGET_DOWNLOAD_HARD_MS`: hard budget window in ms (0 = unbounded).
- `ARW_BUDGET_SOFT_DEGRADE_PCT`: percentage of soft budget used before a “degraded” status is emitted (default `80`).
- `ARW_DL_MIN_MBPS`: minimum expected throughput used for admission checks when total size is known (default `2.0`).
- `ARW_DL_SEND_RETRIES`: HTTP request retries for initial send before failing (default `2`).
- `ARW_DL_STREAM_RETRIES`: stream read retries (resume with Range) before failing (default `2`).
- `ARW_DL_IDLE_TIMEOUT_SECS`: idle fallback timeout when no hard budget is set (default `300`; set `0` to disable).
- `ARW_DL_EWMA_ALPHA`: smoothing factor for throughput EWMA used in admission decisions (default `0.3`).
- `ARW_DL_PREFLIGHT`: when `1`, perform a HEAD preflight to capture `Content-Length` and resume validators (ETag/Last-Modified). Enables early enforcement of `ARW_MODELS_MAX_MB` and `ARW_MODELS_QUOTA_MB` before starting the transfer.
- `ARW_DL_PROGRESS_INCLUDE_BUDGET`: when `1`, include a `budget` snapshot in `Models.DownloadProgress` events.
- `ARW_DL_PROGRESS_INCLUDE_DISK`: when `1`, include a `disk` snapshot `{available,total,reserve}` in progress events.
 
HTTP client (downloads)
- `ARW_DL_HTTP_KEEPALIVE_SECS`: TCP keepalive seconds for the download client pool (default `60`; `0` = unset/OS default).
- `ARW_DL_HTTP_POOL_IDLE_SECS`: idle timeout seconds for pooled connections (default `90`; `0` = unset/disable explicit idle timeout).
- `ARW_DL_HTTP_POOL_MAX_IDLE_PER_HOST`: max idle connections per host (default `8`, min `1`).
The enhanced downloader path is always enabled; the legacy `ARW_DL_NEW` flag has been removed to reduce maintenance overhead.

## Hardware Probes & Metrics
- `ARW_ROCM_SMI`: `1` enables ROCm SMI enrichment for AMD GPU metrics on Linux (best‑effort).
- `ARW_DXCORE_NPU`: `1` enables DXCore probe for NPUs on Windows when built with `npu_dxcore` feature.
- `ARW_METRICS_INTERVAL_SECS`: background SSE `Probe.Metrics` interval seconds (default `10`, min `2`).

## CORS & Networking
- `ARW_CORS_ANY`: `1` to relax CORS during development only.

### Network Posture & Egress (Planned)
These options are planned for the policy‑backed egress gateway; names may evolve during implementation.
- `ARW_NET_POSTURE`: network posture per project: `off|public|allowlist|custom`.
- `ARW_EGRESS_PROXY_ENABLE`: `1` to enable a host‑local egress proxy per node.
- `ARW_EGRESS_PROXY_PORT`: listen port for the local proxy (default `9080`).
- `ARW_EGRESS_BLOCK_IP_LITERALS`: `1` to disallow IP‑literal CONNECTs (require named hosts).
- `ARW_DNS_GUARD_ENABLE`: `1` to force tool DNS through a local resolver; block UDP/53 and DoH/DoT from tools.
- `ARW_DISABLE_HTTP3`: `1` to disable HTTP/3 for headless scrapers, ensuring proxy enforcement.
- `ARW_EGRESS_LEDGER`: path to append‑only egress ledger (default `state://egress.jsonl`).
- `ARW_EGRESS_LEDGER_ENABLE`: `1` to append entries to the egress ledger (opt‑in).

### Security Posture & Mitigations (Planned)
- `ARW_SECURITY_POSTURE`: per‑project preset `relaxed|standard|strict`.
- `ARW_BROWSER_DISABLE_SW`: `1` to disable service workers in headless browsing tools.
- `ARW_BROWSER_SAME_ORIGIN`: `1` to enforce same‑origin fetches by default (allowlists widen).
- `ARW_ARCHIVE_MAX_DEPTH`: max allowed nested archive depth (default `2`).
- `ARW_ARCHIVE_MAX_BYTES`: max total uncompressed bytes per extraction (default `512 MiB`).
- `ARW_DNS_RATE_LIMIT`: max DNS QPS per tool/process (default tuned for local dev).
- `ARW_GPU_ZERO_ON_RELEASE`: `1` to zero VRAM/workspace buffers between jobs when supported.

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
