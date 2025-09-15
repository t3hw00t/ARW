Title: feat(svc+ops): CSP + request IDs, proxy‑aware IP, structured/rolling access logs, Docker/Helm polish

Microsummary
- Add durable middleware (request IDs, security headers, Server‑Timing) and proxy‑aware IP for admin rate‑limits. Introduce CSP presets/auto injection for HTML. Add structured access logs with optional UA/Referer and a rolling file sink. Polish Docker image/Compose/Helm and docs.

Plan (final)
- Service: apps/arw-svc/src/main.rs (middlewares, CSP helpers, SSE request_id, trace span).
- Telemetry: crates/arw-otel (rolling appender layer filtered to http.access).
- Ops: Dockerfile healthcheck/specs; Helm values/env wiring; systemd units; Just helper.
- Docs: Docker, Kubernetes, Reverse Proxy, Security Hardening, Systemd; README quickstart.

Changes
- Request IDs: `x-request-id` middleware; added to traces and SSE initial payload.
- Security headers: nosniff, no-referrer, DENY, conservative permissions policy.
- CSP: `ARW_CSP_AUTO` (default on), presets (`relaxed|strict`), explicit `ARW_CSP` override; applied to landing/admin/spec.
- Server-Timing: `total;dur=ms` header on all responses.
- Proxy‑aware IP: use socket addr by default; honor forwarded headers only with `ARW_TRUST_FORWARD_HEADERS=1`.
- Access logs: `http.access` target with sampling + fields; optional UA (hash) and Referer (strip QS).
- Rolling logs: file rotation via `tracing-appender`; `ARW_ACCESS_LOG_ROLL=1` and `ARW_ACCESS_LOG_DIR/PREFIX/ROTATION`.
- Docker: multi‑stage build, slim runtime, healthcheck, copy `spec/` + `interfaces/`.
- Helm: values and template envs for trust, CSP, access logs (structured + rolling).
- Just: `access-tail` helper to follow latest rolled file.
- CI: new `docker-publish.yml` to build/push multi‑arch images to GHCR.

Docs impact
- Docker (rolling logs), Kubernetes (log flags), Reverse Proxy (trust headers), Security Hardening (headers, CSP, structured + rolling logs), Systemd Service; README Docker quickstart.

Test results
- `cargo build --workspace --release`: OK
- `cargo nextest run -p arw-svc`: 53/53 pass
- Docker image builds/runs; `/healthz` and `/spec/health` OK.
- Helm template validates; env keys present on Pod spec.

Risks / user impact
- CSP strict preset may disable inline scripts on small HTML pages; default remains relaxed.
- Trust of proxy headers is opt‑in; default remains socket addr to avoid spoofing.
- Access logs are opt‑in and sampling defaults to 1 (every request) when enabled; rolling sink gated separately.

Breaking changes
- None (HTTP routes unchanged; headers added are conservative). CSP auto can be disabled via env if needed.

