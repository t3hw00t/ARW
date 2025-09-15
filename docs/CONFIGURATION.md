---
title: Configuration
---

# Configuration
{ .topic-trio style="--exp:.7; --complex:.5; --complicated:.3" data-exp=".7" data-complex=".5" data-complicated=".3" }
Updated: 2025-09-15
Type: Reference

See also: [Glossary](GLOSSARY.md), [Admin Endpoints](guide/admin_endpoints.md), [Quickstart](guide/quickstart.md)

Centralized reference for ARW environment variables and common flags. Defaults favor local, private, and portable operation.

## Service
- `ARW_PORT`: HTTP listen port (default: `8090`).
- `ARW_BIND`: HTTP bind address (default: `127.0.0.1`). Use `0.0.0.0` to listen on all interfaces in trusted environments or behind a TLS proxy.
- `ARW_PORTABLE`: `1` keeps state/cache/logs near the app bundle.
 - `ARW_CONFIG`: absolute path to the primary config TOML (overrides discovery).
 - `ARW_CONFIG_DIR`: base directory to search for additional configs (e.g., `configs/gating.toml`, `configs/feedback.toml`). When unset, the service also probes beside the executable and the current directory.
 - `ARW_KERNEL_ENABLE`: enable the SQLite journal/CAS kernel (default `1`). When enabled, the service dual‑writes events to the kernel and exposes `/triad/events?replay=N`.
- `ARW_ACTIONS_QUEUE_MAX`: backpressure limit for queued actions (default `1024`). When exceeded, `/actions` returns 429.
- `ARW_HTTP_MAX_CONC`: global HTTP concurrency limit (default `1024`) applied via Tower layer. Prevents overload and enforces fairness across routes. SSE `/events` is not limited by timeouts but does count toward concurrency.

## Performance Presets
- `ARW_PERF_PRESET`: selects built‑in runtime tuning presets. Options: `eco|balanced|performance|turbo`. When unset, ARW auto‑detects a tier from CPU cores and RAM and seeds sane defaults.
- `ARW_PERF_PRESET_TIER`: read‑only effective tier after auto‑detection or explicit selection (`eco|balanced|performance|turbo`).

Presets seed defaults for hot‑path tunables if you haven’t set them explicitly:
- `ARW_HTTP_MAX_CONC`: HTTP concurrency limit.
- `ARW_ACTIONS_QUEUE_MAX`: max queued actions before 429.
- `ARW_CONTEXT_SCAN_LIMIT`: max files scanned by `/context/assemble`.
- `ARW_REHYDRATE_FILE_HEAD_KB`: preview bytes for `/context/rehydrate`.
- `ARW_ROUTE_STATS_*`: coalesce/publish cadences for route stats.
- `ARW_MODELS_METRICS_*`: coalesce/publish cadences for models metrics.

Notes
- Explicit env vars always win over presets.
- Presets focus on latency stability and predictable resource use across common laptops/workstations. Use `turbo` on high‑core dev machines, `eco` on low‑power or shared hosts.
- Build profiles already offer `release` vs `maxperf` for build‑time tuning; presets target runtime behavior.

## Admin & Security
- `ARW_ADMIN_TOKEN`: required token for admin endpoints.
 - `ARW_TOOLS_CACHE_TTL_SECS`: Action Cache TTL (seconds; default 600).
 - `ARW_TOOLS_CACHE_CAP`: Action Cache max entries (default 2048).
 - `ARW_ROUTE_STATS_COALESCE_MS`: coalesce window for route stats read‑model patches (default 250ms; min 10ms).
 - `ARW_ROUTE_STATS_PUBLISH_MS`: idle publish cadence for route stats (default 2000ms; min 200ms).
 - `ARW_MODELS_METRICS_COALESCE_MS`: coalesce window for models metrics patches (default 250ms; min 10ms).
 - `ARW_MODELS_METRICS_PUBLISH_MS`: idle publish cadence for models metrics (default 2000ms; min 200ms).
- `ARW_ADMIN_RL`: admin rate limit as `limit/window_secs` (default `60/60`).
- `ARW_DEBUG`: `1` enables local debug mode; do not use in production.
 - `ARW_SECURITY_POSTURE`: posture preset `relaxed|standard|strict`. If no `ARW_POLICY_FILE` is provided, ARW derives a default policy from this. Default is `standard`.
 - `ARW_SCHEMA_MAP`: path to a JSON file that maps top‑level config segments to JSON Schemas for Patch Engine validation (defaults to `configs/schema_map.json`). Example: `{ "recipes": { "schema_ref": "spec/schemas/recipe_manifest.json", "pointer_prefix": "recipes" } }`

## Docs & Debug UI
- `ARW_DOCS_URL`: URL to your hosted docs for UI links. Appears in `GET /about` as `docs_url` so clients can discover your manual.
- Debug UI is accessible at `/admin/debug` when enabled. In local debug builds (`ARW_DEBUG=1`) a friendly alias is also served at `/debug`.
- `ARW_EVENTS_SSE_MODE`: format for SSE `data` payloads. `envelope` (default) emits the ARW envelope `{ time, kind, payload, ce }`. `ce-structured` emits CloudEvents 1.0 structured JSON with `data` holding the payload.
 - `ARW_EVENTS_JOURNAL`: optional path to a JSONL events journal for local replay/inspection.
- `ARW_EVENTS_JOURNAL_MAX_MB`: rotate/journal size cap in MiB (default `20`).
- `ARW_REHYDRATE_FILE_HEAD_KB`: max head bytes when rehydrating local files via `/context/rehydrate` (default `64`).

## Observability & Logs
- `ARW_OTEL=1`: enable OpenTelemetry initialization (preview; pipeline placeholder logs a warning until configured with exporters).
- `ARW_ACCESS_LOG_ROLL=1`: enable rolling access logs filtered to `http.access` target.
  - `ARW_ACCESS_LOG_DIR`: directory for rolled logs (default `${ARW_LOGS_DIR:-./logs}`)
  - `ARW_ACCESS_LOG_PREFIX`: file prefix (default `http-access`)
  - `ARW_ACCESS_LOG_ROTATION`: `daily|hourly|minutely` (default `daily`)

## State & Paths
- `ARW_STATE_DIR`: override state directory.
- `ARW_LOGS_DIR`: override logs directory.
- Connectors metadata and tokens are stored under `${ARW_STATE_DIR}/connectors/*.json`.

Defaults
- Windows: per-user Known Folders via `directories` (e.g., LocalAppData for data/logs, Roaming for config). No writes to Program Files/HKLM by default.
- Unix: XDG‑compatible locations (e.g., `~/.local/share`, `~/.cache`, `~/.config`).

Config discovery (CWD‑independent)
- Primary config: if `ARW_CONFIG` is not set, ARW looks for `configs/default.toml` in the following locations (first hit wins): `ARW_CONFIG_DIR`, beside the executable, parent of the executable (useful in dev trees), repository root (dev), then the current directory.
- Optional configs (e.g., `configs/gating.toml`, `configs/feedback.toml`) follow the same search order via `ARW_CONFIG_DIR` and executable‑relative paths.

## Chat & Models
- `ARW_LLAMA_URL`: llama.cpp server endpoint (e.g., `http://127.0.0.1:8080`).
- `ARW_OPENAI_API_KEY`: OpenAI‑compatible API key.
- `ARW_OPENAI_BASE_URL`: custom base URL for OpenAI‑compatible servers.
- `ARW_OPENAI_MODEL`: default model name when using OpenAI‑compatible backend.
 - `ARW_HTTP_TIMEOUT_SECS`: HTTP client timeout in seconds (default `20`) for built‑in effectors.
 - `ARW_HTTP_BODY_HEAD_KB`: number of KB of response body retained in memory for previews (default `64`).
 - `ARW_NET_ALLOWLIST`: comma‑separated hostnames allowed for HTTP effectors (optional).
 - `ARW_LITELLM_BASE_URL`: LiteLLM server base URL (OpenAI‑compatible). When set, it takes precedence over `ARW_OPENAI_BASE_URL`.
 - `ARW_LITELLM_API_KEY`: API key for LiteLLM (optional; send only if set).
 - `ARW_LITELLM_MODEL`: model name for LiteLLM (falls back to `ARW_OPENAI_MODEL` if unset).
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
- `ARW_DL_PROGRESS_INCLUDE_BUDGET`: when `1`, include a `budget` snapshot in `models.download.progress` events.
- `ARW_DL_PROGRESS_INCLUDE_DISK`: when `1`, include a `disk` snapshot `{available,total,reserve}` in progress events.
- `ARW_DL_PROGRESS_VALIDATE`: when `1`, validate progress `status`/`code` against the known vocabulary and log warnings for unknown values (helps catch drift).
 
HTTP client (downloads)
- `ARW_DL_HTTP_KEEPALIVE_SECS`: TCP keepalive seconds for the download client pool (default `60`; `0` = unset/OS default).
- `ARW_DL_HTTP_POOL_IDLE_SECS`: idle timeout seconds for pooled connections (default `90`; `0` = unset/disable explicit idle timeout).
- `ARW_DL_HTTP_POOL_MAX_IDLE_PER_HOST`: max idle connections per host (default `8`, min `1`).
The enhanced downloader path is always enabled; the legacy `ARW_DL_NEW` flag has been removed to reduce maintenance overhead.

### Interactive Performance Budgets & Streaming

These knobs prioritize perceived latency and streaming cadence.

- `ARW_SNAPPY_I2F_P95_MS`: p95 interaction-to-first-feedback target (default `50`)
- `ARW_SNAPPY_FIRST_PARTIAL_P95_MS`: p95 first useful partial target (default `150`)
- `ARW_SNAPPY_CADENCE_MS`: steady stream cadence budget (default `250`)
- `ARW_SNAPPY_COLD_START_MS`: cold start budget for control plane (default `500`)
- `ARW_SNAPPY_FULL_RESULT_P95_MS`: p95 full result target (default `2000`)
- `ARW_SNAPPY_PROTECTED_ENDPOINTS`: CSV prefixes for interactive surface (default `/debug,/state/,/chat/,/admin/events`)
- `ARW_ROUTE_HIST_MS`: CSV millisecond buckets for route latency histograms (default `5,10,25,50,100,200,500,1000,2000,5000,10000`)
- `ARW_NATS_URL`: NATS URL, e.g. `nats://127.0.0.1:4222`
- `ARW_NODE_ID`: node identifier for NATS subjects (defaults to hostname)
- `ARW_NATS_OUT`: when `1`, relay local events to NATS subjects
- `ARW_NATS_TLS`: when `1`, upgrade `nats://` to `tls://` and `ws://` to `wss://`
- `ARW_NATS_USER` / `ARW_NATS_PASS`: basic auth; injected into URL if not present
- `ARW_NATS_CONNECT_RETRIES`: initial connect retry count (default 3)
- `ARW_NATS_CONNECT_BACKOFF_MS`: initial connect backoff between attempts (default 500 ms)
- `ARW_SNAPPY_PUBLISH_MS`: interactive read‑model publish interval ms (default `2000`)
- `ARW_SNAPPY_DETAIL_EVERY`: seconds between detailed p95 breakdown events (optional)

SSE contract: see `architecture/sse_patch_contract.md` for `Last-Event-ID` and JSON Patch topics.

## Events & Kinds
- Kinds are normalized lowercase dot.case (e.g., `models.download.progress`).
- Normalized kinds appear in the CloudEvents `ce.type` and envelope `kind` fields.
- SSE filters should use normalized prefixes (e.g., `?prefix=models.`).

## Hardware Probes & Metrics
- `ARW_ROCM_SMI`: `1` enables ROCm SMI enrichment for AMD GPU metrics on Linux (best‑effort).
- `ARW_DXCORE_NPU`: `1` enables DXCore probe for NPUs on Windows when built with `npu_dxcore` feature.
- `ARW_METRICS_INTERVAL_SECS`: background SSE `probe.metrics` interval seconds (default `10`, min `2`).

## CORS & Networking
- `ARW_CORS_ANY`: `1` to relax CORS during development only.

### Network Posture & Egress (Planned)
These options control the policy‑backed egress gateway; some are implemented as noted.
- `ARW_NET_POSTURE`: network posture per project: `off|public|allowlist|custom`.
- `ARW_EGRESS_PROXY_ENABLE`: `1` to enable a host‑local egress proxy per node. (preview forward proxy)
- `ARW_EGRESS_PROXY_PORT`: listen port for the local proxy (default `9080`).
- `ARW_EGRESS_BLOCK_IP_LITERALS`: `1` to disallow IP‑literal hosts (require named hosts) for built‑in effectors. (implemented for `http.fetch`)
 - `ARW_DNS_GUARD_ENABLE`: `1` to guard DNS egress: proxy blocks DoH/DoT (`dns.google`, `cloudflare-dns.com`, port `853`), `/dns-query` paths, and `application/dns-message` payloads. Headless tools route via the proxy when enabled.
- `ARW_DISABLE_HTTP3`: `1` to disable HTTP/3 for headless scrapers, ensuring proxy enforcement.
- `ARW_EGRESS_LEDGER`: path to append‑only egress ledger (default `state://egress.jsonl`).
- `ARW_EGRESS_LEDGER_ENABLE`: `1` to append entries to the egress ledger (opt‑in). (implemented)

### Security Posture & Mitigations (Planned)
- `ARW_SECURITY_POSTURE`: per‑project preset `relaxed|standard|strict`.
- `ARW_BROWSER_DISABLE_SW`: `1` to disable service workers in headless browsing tools.
- `ARW_BROWSER_SAME_ORIGIN`: `1` to enforce same‑origin fetches by default (allowlists widen).
- `ARW_ARCHIVE_MAX_DEPTH`: max allowed nested archive depth (default `2`).
- `ARW_ARCHIVE_MAX_BYTES`: max total uncompressed bytes per extraction (default `512 MiB`).
- `ARW_DNS_RATE_LIMIT`: max DNS QPS per tool/process (default tuned for local dev).
- `ARW_GPU_ZERO_ON_RELEASE`: `1` to zero VRAM/workspace buffers between jobs when supported.

## Launcher & CLI
- `ARW_NO_LAUNCHER`: `1` to skip launching the desktop launcher when starting the service.
- `ARW_NO_TRAY`: deprecated alias for `ARW_NO_LAUNCHER` (still honored).
- `ARW_HEADLESS`: `1` for headless setup flows in CI.

See also: CLI Guide (guide/cli.md)

## Trust & Policy
 - `ARW_TRUST_CAPSULES`: path to trusted capsule issuers/keys JSON.
 - `ARW_POLICY_FILE`: JSON file for the ABAC facade (see Guide → Policy (ABAC Facade)). Shape:
   - `{ "allow_all": true|false, "lease_rules": [ { "kind_prefix": "net.http.", "capability": "net:http" } ] }`
   - Presets provided in‑repo: `configs/policy/relaxed.json`, `configs/policy/standard.json`, `configs/policy/strict.json`. Point `ARW_POLICY_FILE` at one of these to mirror `ARW_SECURITY_POSTURE` explicitly.
  - `ARW_GUARDRAILS_URL`: optional base URL for an HTTP guardrails service exposing `POST /check` (tool `guardrails.check`).
  - `ARW_GUARDRAILS_ALLOWLIST`: comma‑separated hostnames considered safe for URL checks (e.g., `example.com, arxiv.org`).

## Tuning Hints
- `ARW_HTTP_TIMEOUT_SECS`: hint for HTTP timeouts used by components that support it.
 - Downloader persists a lightweight throughput EWMA in `{state_dir}/downloads.metrics.json` to improve admission checks across runs.

## Context & Snappy Defaults
- `ARW_CONTEXT_SCAN_LIMIT`: max files scanned in `/context/assemble` (default `200`).
- `ARW_REHYDRATE_FILE_HEAD_KB`: head bytes returned for file rehydrate (default `64`).

## Notes
- Sensitive routes include `/admin/*`, `/debug`, `/probe`, `/memory*`, `/models*`, `/governor*`, `/introspect*`, `/chat*`, `/feedback*`.
- Prefer keeping the service bound to `127.0.0.1` or behind a TLS‑terminating reverse proxy.
## Egress Settings (Config Block)
- Persisted settings can live under the top‑level `egress` block and are validated against `spec/schemas/egress_settings.json` via the Patch Engine.

Example (configs/default.toml)
```
[egress]
posture = "standard"
allowlist = ["api.github.com"]
block_ip_literals = true
dns_guard_enable = true
proxy_enable = true
proxy_port = 9080
ledger_enable = true
```

Runtime overrides
- `POST /egress/settings` updates toggles and persists a snapshot; the proxy is started/stopped immediately.
