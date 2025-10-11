---
title: Configuration
---

# Configuration
{ .topic-trio style="--exp:.7; --complex:.5; --complicated:.3" data-exp=".7" data-complex=".5" data-complicated=".3" }
Updated: 2025-10-11
Type: Reference

See also: [Glossary](GLOSSARY.md), [Admin Endpoints](guide/admin_endpoints.md), [Quickstart](guide/quickstart.md)

Centralized reference for ARW environment variables and common flags. Defaults favor local, private, and portable operation.

## Service
- `ARW_PORT`: HTTP listen port (default: `8091`).
- `ARW_BIND`: HTTP bind address (default: `127.0.0.1`). Use `0.0.0.0` to listen on all interfaces in trusted environments or behind a TLS proxy. The server refuses to start if bound to a non‑loopback address without an admin token (see `ARW_ADMIN_TOKEN`), to avoid accidental public exposure.
- `ARW_PORTABLE`: `1` keeps state/cache/logs near the app bundle.
 - `ARW_CONFIG`: absolute path to the primary config TOML (overrides discovery).
- `ARW_CONFIG_DIR`: base directory to search for additional configs (e.g., `configs/gating.toml`, `configs/feedback.toml`). When unset, the service also probes beside the executable and the current directory.
- `ARW_GATING_FILE`: optional absolute or relative path to the immutable gating policy TOML. Defaults to the discovery chain described below.
- `ARW_KERNEL_ENABLE`: enable the SQLite journal/CAS kernel (default `1`). When enabled, the service dual‑writes events to the kernel and exposes `/events?replay=N`. When disabled (`0`/`false`), journaling and replay endpoints fall back to in-memory delivery only and `/events?replay` returns `501 Not Implemented`.
- `ARW_SQLITE_POOL_SIZE`: starting target for SQLite connections in the pool (default `8`). Requests beyond the current limit block until a handle is returned.
- `ARW_SQLITE_POOL_MIN`: lower bound for the autotuner/shrinker (default `2`).
- `ARW_SQLITE_POOL_MAX`: absolute ceiling for pool expansion (default `32`).
- `ARW_SQLITE_BUSY_MS`: busy timeout applied to each SQLite handle before returning `SQLITE_BUSY` (default `5000`).
- `ARW_SQLITE_CACHE_PAGES`: cache size pragma expressed in pages (default `-20000`, which lets SQLite size the cache relative to available memory).
- `ARW_SQLITE_MMAP_MB`: optional mmap window in MiB. Values ≤ `0` disable the setting; positive values are converted to bytes and passed to `PRAGMA mmap_size`.
- `ARW_SQLITE_CHECKPOINT_SEC`: when set to a positive integer, spawns a background WAL checkpoint loop that runs every `N` seconds using `PRAGMA wal_checkpoint(TRUNCATE)`.
- `ARW_SQLITE_POOL_AUTOTUNE`: set to `1` to enable adaptive tuning of the pool target based on observed wait times (default `0`).
- `ARW_SQLITE_POOL_AUTOTUNE_INTERVAL_SEC`: evaluation interval for the autotuner (default `30`).
- `ARW_SQLITE_POOL_AUTOTUNE_WAIT_MS`: average wait threshold (in ms) that triggers pool growth (default `50`). Shrink decisions use one quarter of this threshold.
- `ARW_SPEC_DIR`: base directory for spec artifacts served under `/spec/*` (default: `spec`).
 - `ARW_INTERFACES_DIR`: base directory for the interface catalog served at `/catalog/index` (default: `interfaces`).
- `ARW_ACTIONS_QUEUE_MAX`: backpressure limit for queued actions. Defaults follow the active performance preset (256 → 16384); explicit exports override the preset.
- `ARW_WORKERS`: number of background action workers that drain the queue. Defaults to roughly twice the detected host parallelism (capped at 32) when unset; set explicitly to pin the pool size.
- `ARW_WORKERS_MAX`: optional ceiling applied to the auto-detected worker count (ignored when `ARW_WORKERS` is set).
- `ARW_MEMORY_EMBED_BACKFILL_BATCH`: number of rows to process per embedding backfill pass (default `256`). Set to `0` to disable the background task.
- `ARW_MEMORY_EMBED_BACKFILL_IDLE_SEC`: idle sleep between backfill passes when no rows are updated or an error occurs (default `300` seconds).
- `ARW_ACTION_STAGING_MODE`: staging policy for `/actions` submissions. Options: `auto` (default, queue immediately), `ask` (stage unless action kind appears in `ARW_ACTION_STAGING_ALLOW`), or `always` (stage every action for manual approval).
- `ARW_ACTION_STAGING_ALLOW`: comma‑delimited list of action kinds that bypass staging when `ARW_ACTION_STAGING_MODE=ask`.
- `ARW_ACTION_STAGING_ACTOR`: label recorded on staging entries for audit trails (defaults to `local`).
- `ARW_HTTP_MAX_CONC`: global HTTP concurrency limit seeded by the performance preset (256 → 16384). Prevents overload and enforces fairness across routes. SSE `/events` is not limited by timeouts but counts toward concurrency.

## Performance Presets
- `ARW_PERF_PRESET`: selects built‑in runtime tuning presets. Options: `eco|balanced|performance|turbo`. When unset, ARW auto‑detects a tier from CPU cores and RAM and seeds sane defaults.
- `ARW_PERF_PRESET_TIER`: read‑only effective tier after auto‑detection or explicit selection (`eco|balanced|performance|turbo`).

Presets seed defaults for hot-path tunables if you haven’t set them explicitly:
- `ARW_HTTP_MAX_CONC`: HTTP concurrency limit.
- `ARW_ACTIONS_QUEUE_MAX`: max queued actions before 429.
- `ARW_CONTEXT_K`: target size of the working set returned by `/context/assemble`.
- `ARW_CONTEXT_EXPAND_PER_SEED`: number of memory links expanded per seed item during context assembly.
- `ARW_CONTEXT_DIVERSITY_LAMBDA`: diversity weighting (MMR lambda) applied to the working set selector.
- `ARW_CONTEXT_MIN_SCORE`: minimum coherence score required for an item to remain in the working set.
- `ARW_CONTEXT_LANES_DEFAULT`: default lanes (CSV) consulted when building the working set.
- `ARW_CONTEXT_LANE_BONUS`: preference bonus applied when a lane has not yet been selected in the current working set.
- `ARW_CONTEXT_EXPAND_QUERY`: enable pseudo-relevance feedback for query expansion during working-set assembly (`0|1`).
- `ARW_CONTEXT_EXPAND_QUERY_TOP_K`: number of top seeds considered when synthesizing the expansion embedding.
- `ARW_CONTEXT_SCORER`: working-set scorer (`mmrd`, `confidence`, or custom implementations).
- `ARW_CONTEXT_STREAM_DEFAULT`: enable SSE streaming by default for `/context/assemble` (`0|1`).
- `ARW_CONTEXT_SLOT_BUDGETS`: optional slot caps for the working set; accepts JSON (`{"instructions":2}`) or comma pairs (`instructions=2,plan=3`).
- `ARW_RESEARCH_WATCHER_SEED`: optional path to a JSON file containing seed suggestions for the Research Watcher (`[{ "source_id": ..., "title": ... }]`).
- `ARW_RESEARCH_WATCHER_FEEDS`: comma-separated list of HTTP(S) endpoints returning Research Watcher items in JSON (`{ "items": [...] }` or `[ ... ]`).
- `ARW_RESEARCH_WATCHER_INTERVAL_SECS`: poll interval for Research Watcher feeds (default `900`, minimum `300`).
- `ARW_CONTEXT_COVERAGE_MAX_ITERS`: maximum iterations allowed for the coverage (CRAG) refinement loop.
- `ARW_REHYDRATE_FILE_HEAD_KB`: preview bytes for `/context/rehydrate`.

## Memory overlay
- `ARW_MEMORY_GC_INTERVAL_SECS`: cadence (seconds) for the background memory hygiene sweep (default `60`).
- `ARW_MEMORY_GC_BATCH`: maximum number of records reclaimed per sweep across TTL expiry and lane overflow (default `128`).
- `ARW_MEMORY_LANE_CAPS`: comma-separated `lane=cap` overrides for per-lane retention. Example: `ephemeral=200,episodic=1500,semantic=6000`. Setting a cap to `0` removes the limit for that lane. Defaults are `ephemeral=256`, `episodic=1024`, `semantic=4096`, `profile=512`.
- `ARW_MEMORY_SHORT_TTL_SECS`: TTL (seconds) applied to modular stack short-term turn buffers (defaults to `900`, i.e., 15 minutes). Increase for longer replay windows; decrease for extra-private deployments.

Preset heuristics today:

| Preset | `ARW_HTTP_MAX_CONC` | `ARW_ACTIONS_QUEUE_MAX` |
| --- | --- | --- |
| `eco` | 256 | 256 |
| `balanced` | 1024 | 1024 |
| `performance` | 4096 | 4096 |
| `turbo` | 16384 | 16384 |

The tier is auto-detected at startup unless you set `ARW_PERF_PRESET`; values only seed defaults and can be overridden per environment.
- `ARW_ROUTE_STATS_*`: coalesce/publish cadences for route stats.
- `ARW_MODELS_METRICS_*`: coalesce/publish cadences for models metrics.

Notes
- Explicit env vars always win over presets.
- Presets focus on latency stability and predictable resource use across common laptops/workstations. Use `turbo` on high‑core dev machines, `eco` on low‑power or shared hosts.
- Build profiles already offer `release` vs `maxperf` for build‑time tuning; presets target runtime behavior.

## Admin & Security
- `ARW_ADMIN_TOKEN`: required token for admin endpoints; when set, also required for `/events` and sensitive `/state/*` endpoints. If no token is configured, set `ARW_DEBUG=1` for local access—otherwise admin routes return `401`.
- `ARW_ADMIN_TOKEN_SHA256`: hex‑encoded SHA‑256 of the admin token. Prefer this in environments where passing plaintext envs is undesirable. When both are set, either value is accepted.
  - Generate hashes locally with `arw-cli admin token hash --token 'plain-secret'` (omit `--token` to read `ARW_ADMIN_TOKEN`, or add `--stdin` to supply the secret via standard input). Append `--env` or `--export-shell` when you want ready-to-paste `ARW_ADMIN_TOKEN_SHA256=…` entries.
  - Generate fresh tokens with `arw-cli admin token generate --length 32` (defaults shown). Use `--format base64` for unpadded base64 output, `--uppercase` for capitalised hex, and `--env`/`--export-shell` when you want ready-to-paste `ARW_ADMIN_TOKEN` assignments. Add `--hash`, `--hash-env`, or `--hash-export-shell` to emit the matching `ARW_ADMIN_TOKEN_SHA256` line alongside the token.
- `ARW_TOOLS_CACHE_TTL_SECS`: Action Cache TTL (seconds; default 600).
- `ARW_TOOLS_CACHE_CAP`: Action Cache max entries (default 2048).
- `ARW_TOOLS_CACHE_MAX_PAYLOAD_BYTES`: Maximum JSON payload size stored per entry (default 4 MiB). Accepts human-friendly suffixes (`512kb`, `8MB`, `2gb`); set to `0`, `off`, or `disabled` to turn the cap off when absolutely necessary.
 - `ARW_ROUTE_STATS_COALESCE_MS`: coalesce window for route stats read‑model patches (default 250ms; min 10ms).
 - `ARW_ROUTE_STATS_PUBLISH_MS`: idle publish cadence for route stats (default 2000ms; min 200ms).
 - `ARW_MODELS_METRICS_COALESCE_MS`: coalesce window for models metrics patches (default 250ms; min 10ms).
 - `ARW_MODELS_METRICS_PUBLISH_MS`: idle publish cadence for models metrics (default 2000ms; min 200ms).
- `ARW_ADMIN_RATE_LIMIT`: per token/IP request budget inside the sliding window (default `60`). Set to `0` to disable throttling.
- `ARW_ADMIN_RATE_WINDOW_SECS`: sliding window length in seconds for the admin rate limiter (default `60`).
- `ARW_DEBUG`: `1` enables local debug mode; do not use in production. When unset, admin routes require a valid `ARW_ADMIN_TOKEN` header.
- `ARW_REFERRER_POLICY`: referrer policy header value (default `no-referrer`).
- `ARW_HSTS`: `1` to enable `Strict-Transport-Security` header (only when served behind HTTPS).
 - `ARW_SECURITY_POSTURE`: posture preset `relaxed|standard|strict`. If no `ARW_POLICY_FILE` is provided, ARW derives a default policy from this. Default is `standard`.
 - `ARW_SCHEMA_MAP`: path to a JSON file that maps top‑level config segments to JSON Schemas for Patch Engine validation (defaults to [`configs/schema_map.json`](https://github.com/t3hw00t/ARW/blob/main/configs/schema_map.json)). Example: `{ "recipes": { "schema_ref": "spec/schemas/recipe_manifest.json", "pointer_prefix": "recipes" } }`

## Events (SSE)
- `ARW_EVENTS_SSE_MODE`: payload format for SSE `data:` frames. Options:
  - `envelope` (default): `{ time, kind, payload }` with optional `ce` metadata
  - `ce-structured`: CloudEvents 1.0 structured JSON (`data` holds the event payload)

## Docs & Debug UI
- `ARW_DOCS_URL`: URL to your hosted docs for UI links. Appears in `GET /about` as `docs_url` so clients can discover your manual.
- Debug UI is accessible at `/admin/debug` when enabled (`ARW_DEBUG=1`).
- `ARW_EVENTS_SSE_MODE`: format for SSE `data` payloads. `envelope` (default) emits the ARW envelope `{ time, kind, payload, ce }`. `ce-structured` emits CloudEvents 1.0 structured JSON with `data` holding the payload.
- Inspect recent entries via `GET /admin/events/journal?limit=200` (requires admin auth). Use `prefix=kind.` CSV filters to scope to specific event families.
- CLI mirror: `arw-cli events journal --limit 200 --prefix memory.` prints a summary without crafting curl calls (`--json` yields raw response; add `--follow --interval 3` to poll continuously; `--after 2025-10-02T17:15:00Z` or `--after-relative 15m` skips older entries on the first fetch; set `--payload-width 0` to hide payloads in shared terminals). Text mode lists an Age column beside each timestamp for quick scanning.
- Modular provenance tail: `arw-cli events modular` follows `modular.agent.accepted` / `modular.tool.accepted` with sensible defaults (limit 100, follow every 3s, payload width 200). Override any flags from `events journal` (for example add `--json` for a single snapshot or `--prefix modular.tool.` to focus on tools).
- Observations snapshot: `arw-cli events observations --limit 50 --kind-prefix service. --since-relative 15m` mirrors `/state/observations` so you can audit recent envelopes without downloading the full window. Text output now includes an Age column for quick triage (`--json` emits the raw read-model; set `--payload-width 0` to hide payload columns in shared terminals, and `--since` or `--since-relative` skips older entries during long investigations).
- Actions snapshot: `arw-cli state actions --state completed --kind-prefix chat.` mirrors `/state/actions` with optional `--limit`, `--updated-since`, `--updated-relative`, and `--watch` flags—pass `--watch` to stay attached to the live `state.read.model.patch` stream instead of polling. The text view surfaces an Age column alongside the updated timestamp so fresh items pop out immediately.
- `ARW_REHYDRATE_FILE_HEAD_KB`: max head bytes when rehydrating local files via `/context/rehydrate` (default `64`).

## Observability & Logs
- `ARW_OTEL=1`: enable the OpenTelemetry OTLP tracing exporter. Optional overrides:
  - `ARW_OTEL_ENDPOINT`: OTLP gRPC endpoint (default `http://127.0.0.1:4317`).
  - `ARW_OTEL_HEADERS`: comma-separated `key=value` list appended as gRPC metadata (useful for API keys).
  - `ARW_OTEL_TIMEOUT_MS`: exporter timeout in milliseconds (default `10000`).
  - `ARW_OTEL_SERVICE_NAME`: override the reported `service.name` (default `arw-server`).
- `ARW_ACCESS_LOG_ROLL=1`: enable rolling access logs filtered to `http.access` target.
  - `ARW_ACCESS_LOG_DIR`: directory for rolled logs (default `${ARW_LOGS_DIR:-./logs}`)
  - `ARW_ACCESS_LOG_PREFIX`: file prefix (default `http-access`)
  - `ARW_ACCESS_LOG_ROTATION`: `daily|hourly|minutely` (default `daily`)
- Logs live under `ARW_LOGS_DIR` (default `./logs`). Tail them with `just access-tail` on Bash-capable systems or `Get-Content -Path .\logs\http-access* -Wait` in PowerShell.
- Kernel emits the following metrics (when the `metrics` feature is enabled, on by default via `arw-server`):
  - Gauges: `arw_kernel_pool_available`, `arw_kernel_pool_in_use`, `arw_kernel_pool_total`.
  - Wait telemetry: `arw_kernel_pool_wait_total` (counter) and `arw_kernel_pool_wait_ms` (histogram, milliseconds).
  - Checkpoint loop counters (enabled when `ARW_SQLITE_CHECKPOINT_SEC` > `0`): `arw_kernel_checkpoint_runs`, `arw_kernel_checkpoint_failures`.
  - Autotune loop counters (enabled when `ARW_SQLITE_POOL_AUTOTUNE=1`): `arw_kernel_pool_autotune_grow`, `arw_kernel_pool_autotune_shrink`.

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
- Gating policy: resolves `ARW_GATING_FILE` first. When unset, it follows the same discovery chain as above for `configs/gating.toml`.

## Chat & Models
- `ARW_LLAMA_URL`: llama.cpp server endpoint (e.g., `http://127.0.0.1:8080`).
- `ARW_LLAMA_N_PREDICT`: override the llama.cpp token budget per request (default `256`, clamped `1-8192`).
- `ARW_LLAMA_TOP_P`: optional nucleus sampling cap forwarded to llama.cpp (`0.0-1.0`).
- `ARW_LLAMA_TOP_K`: optional top-k sampling limit forwarded to llama.cpp (`1-5000`).
- `ARW_LLAMA_MIN_P`: optional minimum token probability forwarded to llama.cpp (`0.0-1.0`).
- `ARW_LLAMA_REPEAT_PENALTY`: optional repetition penalty forwarded to llama.cpp (`0.0-4.0`).
- `ARW_LLAMA_STOP`: comma or newline separated stop sequences forwarded to llama.cpp.
- `ARW_OPENAI_API_KEY`: OpenAI‑compatible API key.
- `ARW_OPENAI_BASE_URL`: custom base URL for OpenAI‑compatible servers.
- `ARW_OPENAI_MODEL`: default model name when using OpenAI‑compatible backend.
- `ARW_OPENAI_MAX_TOKENS`: override requested max tokens (default `512`, clamped `16-4096`).
- `ARW_OPENAI_TOP_P`: override nucleus sampling cap (`0.0-1.0`).
- `ARW_OPENAI_FREQUENCY_PENALTY`: set OpenAI `frequency_penalty` (`-2.0` to `2.0`).
- `ARW_OPENAI_PRESENCE_PENALTY`: set OpenAI `presence_penalty` (`-2.0` to `2.0`).
- `ARW_OPENAI_STOP`: comma or newline separated stop sequences forwarded to the OpenAI-compatible backend.
- `ARW_CHAT_DEFAULT_TEMPERATURE`: default sampling temperature when none is supplied (default `0.2`, clamped `-5.0` to `5.0`).
- `ARW_CHAT_DEFAULT_VOTE_K`: default self-consistency vote count when none is supplied (default `1`, clamped `1-5`).
- `ARW_CHAT_SYSTEM_PROMPT`: optional system prompt prepended to chat requests (default `"You are a helpful assistant."`).
- `ARW_HTTP_TIMEOUT_SECS`: shared HTTP timeout in seconds (default `20`). The unified server seeds this value on startup and governor hints update it live.
- `ARW_HTTP_BODY_HEAD_KB`: number of KB of response body retained in memory for previews (default `64`).
- `ARW_NET_ALLOWLIST`: comma‑separated hostnames allowed for HTTP effectors (optional).
- `ARW_LITELLM_BASE_URL`: LiteLLM server base URL (OpenAI‑compatible). When set, it takes precedence over `ARW_OPENAI_BASE_URL`.
 - `ARW_LITELLM_API_KEY`: API key for LiteLLM (optional; send only if set).
 - `ARW_LITELLM_MODEL`: model name for LiteLLM (falls back to `ARW_OPENAI_MODEL` if unset).
- `ARW_MODELS_MAX_MB`: hard cap for single model download size in MiB (default `4096`). Enforced by the unified server before and during transfers.
- `ARW_MODELS_DISK_RESERVE_MB`: reserve free space during downloads in MiB (default `256`). The unified server aborts downloads if free space drops below this reserve.
- `ARW_MODELS_MAX_CONC`: max concurrent model downloads (default `2`; `0` or `<1` treated as `1`).
- `ARW_MODELS_QUOTA_MB`: optional total on‑disk quota for all models stored in CAS (sum of `state/models/by-hash/*`) in MiB. When set, downloads are denied if projected total would exceed the quota.

### Downloads & Budgets
- `ARW_BUDGET_DOWNLOAD_SOFT_MS`: soft budget window in ms (0 = unbounded).
- `ARW_BUDGET_DOWNLOAD_HARD_MS`: hard budget window in ms (0 = unbounded). When elapsed time reaches this window the unified server aborts the download and emits `models.download.progress` with `status:"error"` and `code:"hard-budget"`.
- `ARW_BUDGET_SOFT_DEGRADE_PCT`: percentage of soft budget used before a “degraded” status is emitted (default `80`).
- `ARW_DL_SEND_RETRIES`: HTTP request retries for initial send before failing (default `2`).
- `ARW_DL_STREAM_RETRIES`: stream read retries (resume with Range) before failing (default `2`).
- `ARW_DL_IDLE_TIMEOUT_SECS`: idle fallback timeout when no hard budget is set (default `300`; set `0` to disable).
- `ARW_DL_RETRY_BACKOFF_MS`: base backoff (in milliseconds) between retry attempts (default `500`; applied linearly per attempt).
- `ARW_DL_PREFLIGHT`: when `1`, perform a HEAD preflight to capture `Content-Length` and resume validators (ETag/Last-Modified). Enables early enforcement of `ARW_MODELS_MAX_MB` and `ARW_MODELS_QUOTA_MB` before starting the transfer. Default `1` (set to `0` to disable).
- `ARW_DL_PROGRESS_INCLUDE_BUDGET`: when `1`, include a `budget` snapshot (soft/hard ms, elapsed, remaining, state) in unified `models.download.progress` events.
- `ARW_DL_PROGRESS_INCLUDE_DISK`: when `1`, include a `disk` snapshot `{reserve,available,need}` (bytes) in unified `models.download.progress` events.

_Forward-looking knobs (not yet wired):_ `ARW_DL_MIN_MBPS`, `ARW_DL_EWMA_ALPHA`, `ARW_DL_PROGRESS_VALIDATE`, `ARW_DL_HTTP_KEEPALIVE_SECS`, `ARW_DL_HTTP_POOL_IDLE_SECS`, and `ARW_DL_HTTP_POOL_MAX_IDLE_PER_HOST` are reserved for upcoming downloader tuning. They are documented here to keep configuration names stable, but the current server ignores them.

### Interactive Performance Budgets & Streaming

These knobs prioritize perceived latency and streaming cadence.

- `ARW_SNAPPY_I2F_P95_MS`: p95 interaction-to-first-feedback target (default `50`)
- `ARW_SNAPPY_FIRST_PARTIAL_P95_MS`: p95 first useful partial target (default `150`)
- `ARW_SNAPPY_CADENCE_MS`: steady stream cadence budget (default `250`)
- `ARW_SNAPPY_COLD_START_MS`: cold start budget for control plane (default `500`)
- `ARW_SNAPPY_FULL_RESULT_P95_MS`: p95 full result target (default `2000`)
- `ARW_SNAPPY_PROTECTED_ENDPOINTS`: CSV prefixes for interactive surface (default `/admin/debug,/state/,/chat/,/events`)
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

_Planned:_ `ARW_METRICS_INTERVAL_SECS` will expose the `probe.metrics` interval once metrics streaming moves out of the debug surfaces.

## CORS, Headers & Networking
- `ARW_CSP_AUTO`: `1` to auto‑inject a CSP for `text/html` responses (default `1`).
- `ARW_CSP_PRESET`: CSP preset `relaxed|strict` (default `relaxed`).
- `ARW_CSP`: explicit CSP policy string; set to `off`/`0` to disable.
- `ARW_TRUST_FORWARD_HEADERS`: `1` to trust `X-Forwarded-For`/`Forwarded` (access log client IP) when behind a trusted proxy.

_Planned:_ `ARW_CORS_ANY` returns once we finish the hardened CORS story for the debug UI and launcher windows. For now CORS remains strict.

### Access Logs (stdout)
- `ARW_ACCESS_LOG`: `1` to enable JSON access logs to stdout.
- `ARW_ACCESS_SAMPLE_N`: sample every Nth request (default `1`).
- `ARW_ACCESS_UA`: `1` to include User‑Agent; `ARW_ACCESS_UA_HASH=1` to include only a SHA‑256 hash.
- `ARW_ACCESS_REF`: `1` to include Referer; `ARW_ACCESS_REF_STRIP_QS=1` to drop the query string.

### Network Posture & Egress (Planned)
These options control the policy‑backed egress gateway; some are implemented as noted.
- `ARW_NET_POSTURE`: network posture per project: `off|public|allowlist|custom`.
- `ARW_EGRESS_PROXY_ENABLE`: `1` to enable a host‑local egress proxy per node (default: `1`). (preview forward proxy)
- `ARW_EGRESS_PROXY_PORT`: listen port for the local proxy (default `9080`).
- `ARW_EGRESS_BLOCK_IP_LITERALS`: `1` to disallow IP‑literal hosts (require named hosts) for built‑in effectors. (implemented for `http.fetch`)
- `ARW_DNS_GUARD_ENABLE`: `1` to guard DNS egress (default: `1`): proxy blocks DoH/DoT (`dns.google`, `cloudflare-dns.com`, port `853`), `/dns-query` paths, and `application/dns-message` payloads. Headless tools route via the proxy when enabled.
- `ARW_DISABLE_HTTP3`: `1` to disable HTTP/3 for headless scrapers, ensuring proxy enforcement.
- `ARW_EGRESS_LEDGER_ENABLE`: `1` to append entries to the egress ledger (opt-in). (implemented)
- `ARW_EGRESS_MULTI_LABEL_SUFFIXES`: comma-separated additional multi-label suffixes (for example `internal.test,gov.bc.ca`) to treat as registrable domains when deriving capability/allowlist checks.

Config file example:

```toml
[egress]
multi_label_suffixes = ["internal.test", "gov.bc.ca"]
```

Each entry should be the registrable suffix (effective TLD) you want treated as a unit; the runtime automatically prepends the immediate owner label when collapsing hostnames.

_Deprecated:_ `ARW_EGRESS_LEDGER` previously pointed to an external JSONL path; ledger entries now live in the kernel and the variable is ignored.

_Deprecated:_ `ARW_EGRESS_LEDGER` used to point at an external JSONL file. The unified server now stores the ledger in the kernel; leave the variable unset.

### Security Posture & Mitigations (Planned)
- `ARW_SECURITY_POSTURE`: per‑project preset `relaxed|standard|strict`.
- `ARW_BROWSER_DISABLE_SW`: `1` to disable service workers in headless browsing tools.
- `ARW_BROWSER_SAME_ORIGIN`: `1` to enforce same‑origin fetches by default (allowlists widen).
- `ARW_ARCHIVE_MAX_DEPTH`: max allowed nested archive depth (default `2`).
- `ARW_ARCHIVE_MAX_BYTES`: max total uncompressed bytes per extraction (default `512 MiB`).
- `ARW_DNS_RATE_LIMIT`: max DNS QPS per tool/process (default tuned for local dev).
- `ARW_GPU_ZERO_ON_RELEASE`: `1` to zero VRAM/workspace buffers between jobs when supported.

These posture toggles remain in backlog until the sandboxing work solidifies; the current builds ignore them.

## Launcher & CLI
- `ARW_NO_LAUNCHER`: `1` to skip launching the desktop launcher when starting the service.
- `ARW_NO_TRAY`: deprecated alias for `ARW_NO_LAUNCHER` (still honored).
- `ARW_HEADLESS`: `1` for headless setup flows in CI.

See also: [CLI Guide](guide/cli.md)

## Managed Runtime Supervisor
- `ARW_RUNTIME_RESTART_MAX`: maximum automatic restarts allowed within the sliding window before the supervisor refuses a new restore (default `3`). Values ≤ `0` fall back to the default; raise the ceiling if you tolerate additional crash loops while debugging.
- `ARW_RUNTIME_RESTART_WINDOW_SEC`: sliding window (seconds) used to evaluate the restart budget (default `600`, i.e., 10 minutes). Combine with `ARW_RUNTIME_RESTART_MAX` to tune how aggressively the supervisor will attempt recoveries on unstable adapters.
- `ARW_RUNTIME_MATRIX_TTL_SEC`: cache window (seconds) for `/state/runtime_matrix` snapshots (default `60`). Values are clamped between `10` and `900` seconds to balance freshness with large-cluster stability and surface back to clients as `ttl_seconds` in the runtime matrix response.

When the budget is exhausted, `/state/runtime_matrix` and `runtime.state.changed` events include a restart quota summary so operators can see when the window resets or expand the limit explicitly.

## Trust & Policy
 - `ARW_TRUST_CAPSULES`: path to trusted capsule issuers/keys JSON.
 - `ARW_POLICY_FILE`: JSON file for the ABAC facade (see Guide → Policy (ABAC Facade)). Shape:
   - `{ "allow_all": true|false, "lease_rules": [ { "kind_prefix": "net.http.", "capability": "net:http" } ] }`
   - Presets provided in-repo: [configs/policy/relaxed.json](https://github.com/t3hw00t/ARW/blob/main/configs/policy/relaxed.json), [configs/policy/standard.json](https://github.com/t3hw00t/ARW/blob/main/configs/policy/standard.json), [configs/policy/strict.json](https://github.com/t3hw00t/ARW/blob/main/configs/policy/strict.json). Point `ARW_POLICY_FILE` at one of these to mirror `ARW_SECURITY_POSTURE` explicitly.
  - `ARW_GUARDRAILS_URL`: optional base URL for an HTTP guardrails service exposing `POST /check` (tool `guardrails.check`).
 - `ARW_GUARDRAILS_ALLOWLIST`: comma‑separated hostnames considered safe for URL checks (e.g., `example.com, arxiv.org`).
 - `ARW_PATCH_SAFETY`: when set to `1`, `true`, or `enforce`, reject config/logic-unit patches that trip the built-in red-team heuristics (permission widening, SSRF markers, prompt-injection bait, secret keywords). When unset, findings are reported in responses and events but do not block writes.
- `ARW_CAPSULE_REFRESH_SECS`: upper bound (seconds) between background capsule sweeps (default `5`).
- `ARW_CAPSULE_REFRESH_MS`: optional millisecond override for the sweep ceiling; use when capsules carry very short leases. When unset, the scheduler derives sub-second sleeps automatically from the earliest lease renewal or hop countdown and clamps to the `ARW_CAPSULE_REFRESH_SECS` ceiling.
- `ARW_CAPSULE_REQUEST_REFRESH_MS`: maximum staleness tolerated on request paths before a best-effort capsule refresh is forced (defaults to the derived sweep ceiling). Relax this for high-throughput deployments once the background refresher is enabled; tighten it for smoke-test setups that rely solely on on-demand refreshes.

## Tuning Hints
- `ARW_HTTP_TIMEOUT_SECS`: runtime-adjustable HTTP timeout applied across built-in HTTP clients; governor hints persist updates back to this environment variable.
- Downloader persists a lightweight throughput EWMA in `{state_dir}/downloads.metrics.json` to improve admission checks across runs.

## Context & Snappy Defaults
- `ARW_CONTEXT_K`: default working set size for `/context/assemble` (preset driven).
- `ARW_CONTEXT_EXPAND_PER_SEED`: link fan-out per seed item when building the working set.
- `ARW_CONTEXT_DIVERSITY_LAMBDA`: diversity weighting applied during working set selection.
- `ARW_CONTEXT_MIN_SCORE`: minimum coherence score required to keep an item in the working set.
- `ARW_CONTEXT_LANES_DEFAULT`: default memory lanes (CSV) used when no lanes are supplied in a request.
- `ARW_CONTEXT_LANE_BONUS`: diversity bonus awarded to lanes that have not yet been represented in the working set.
- `ARW_CONTEXT_EXPAND_QUERY`: toggles pseudo-relevance expansion for hybrid retrieval.
- `ARW_CONTEXT_EXPAND_QUERY_TOP_K`: seeds considered when generating the expansion embedding.
- `ARW_CONTEXT_SCORER`: working-set scorer strategy.
- `ARW_CONTEXT_STREAM_DEFAULT`: default SSE streaming behaviour for `/context/assemble`.
- `ARW_CONTEXT_SLOT_BUDGETS`: default slot caps applied when callers omit `slot_budgets`; accepts JSON (`{"instructions":2}`) or comma pairs (`instructions=2,plan=3`).
- `ARW_CONTEXT_COVERAGE_MAX_ITERS`: upper bound on CRAG refinement passes before returning results.
- `ARW_REHYDRATE_FILE_HEAD_KB`: head bytes returned for file rehydrate (default `64`).

## Notes
- Sensitive routes include `/admin/*`, `/admin/debug`, `/probe`, `/admin/memory*`, `/state/memory*`, `/models*`, `/governor*`, `/introspect*`, `/chat*`, `/feedback*`.
- Prefer keeping the service bound to `127.0.0.1` or behind a TLS‑terminating reverse proxy.
## Egress Settings (Config Block)
- Persisted settings can live under the top‑level `egress` block and are validated against [spec/schemas/egress_settings.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/egress_settings.json) via the Patch Engine.

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
