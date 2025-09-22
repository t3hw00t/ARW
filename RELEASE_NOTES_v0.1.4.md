## Summary

Adds durable request correlation, safer default headers, CSP controls, and richer ops: structured access logs with optional rolling files, SSE request-id correlation, and Docker/Helm polish. No breaking HTTP or SSE changes.

> **Legacy notice:** As of the current `main` branch the legacy bridge has been retired; these notes remain for historical context of the v0.1.4 cut.

## Highlights
- Request IDs: end-to-end `x-request-id` propagation across responses and traces.
- Security Headers: `X-Content-Type-Options`, `Referrer-Policy`, `X-Frame-Options`, conservative `Permissions-Policy`.
- CSP: auto-inject for HTML, presets (`relaxed|strict`), explicit override env. Applied on Landing, Admin Index, Specs.
- Server-Timing: `total;dur=<ms>` header for quick client-side latency checks.
- Proxy-Aware IP: admin rate-limits prefer socket IP; honor `X-Forwarded-For` only when opted-in.
- Access Logs: structured `http.access` target with sampling; optional UA (hash) and Referer (strip QS).
- Rolling Access Logs: file rotation (daily/hourly/minutely) via non-blocking appender.
- SSE Correlation: initial `service.connected` includes `request_id`; optional per-event decoration.
- Docker/Helm: slimmer image with healthcheck and bundled specs; chart values for CSP/proxy/logging.
- Ops: systemd service examples (native/container) and docs; `just access-tail` helper.
- CI: multi-arch image publish to GHCR on tags.

## Changes
- Middleware: request-id, security headers, server-timing, proxy-aware client IP.
- CSP: `ARW_CSP_AUTO` (default on), `ARW_CSP_PRESET=relaxed|strict` (default relaxed), `ARW_CSP` override or disable.
- SSE: add `request_id` to initial event; optional decoration of each event payload when enabled.
- Access logs: sampling and fields (method, path, matched, status, dt_ms, ip, request_id, req_len, resp_len); optional UA/Referer.
- Rolling logs: `http.access` to rolling files, configurable directory/prefix/rotation.
- Dockerfile: multi-stage, `libxcb1`/`curl`, healthcheck, `spec/` and `interfaces/` copied; README quickstart.
- Helm: values expose `ARW_TRUST_FORWARD_HEADERS`, `ARW_ACCESS_*`, `ARW_CSP_*`; deployment envs updated.
- Docs: Docker (with rolling logs), Kubernetes (helm flags), Reverse Proxy (trusted headers), Security Hardening (headers/CSP/logs), Systemd Service.

## Compatibility
- No breaking API/SSE changes. Defaults:
  - `ARW_TRUST_FORWARD_HEADERS=0`: proxy headers not trusted by default.
  - `ARW_CSP_AUTO=1`, `ARW_CSP_PRESET=relaxed`: CSP injected for HTML unless overridden.
  - `ARW_ACCESS_LOG=0`, `ARW_ACCESS_LOG_ROLL=0`: access logs and rolling sink are opt-in.

## New/Updated Env Vars
- Proxy/IP: `ARW_TRUST_FORWARD_HEADERS`
- Access logs: `ARW_ACCESS_LOG`, `ARW_ACCESS_SAMPLE_N`, `ARW_ACCESS_UA`, `ARW_ACCESS_UA_HASH`, `ARW_ACCESS_REF`, `ARW_ACCESS_REF_STRIP_QS`
- Rolling sink: `ARW_ACCESS_LOG_ROLL`, `ARW_ACCESS_LOG_DIR`, `ARW_ACCESS_LOG_PREFIX`, `ARW_ACCESS_LOG_ROTATION`
- CSP: `ARW_CSP_AUTO`, `ARW_CSP_PRESET`, `ARW_CSP`
- SSE decoration: `ARW_EVENTS_SSE_DECORATE`

## Install
- Docker: `ghcr.io/<owner>/arw-server:v0.1.4`
- Helm: use the unified server chart (forthcoming) or mirror the Docker image directly until published.
- Binaries: build from source with `cargo build --workspace --release`

## Verification
- Health: `GET /healthz`
- Specs: `GET /spec/health`
- Admin (token or debug): `GET /admin/probe`
