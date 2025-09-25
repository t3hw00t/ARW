---
title: Security Hardening
---

# Security Hardening
{ .topic-trio style="--exp:.4; --complex:.6; --complicated:.9" data-exp=".4" data-complex=".6" data-complicated=".9" }

This guide summarizes recommended steps to run ARW more securely beyond the local‑dev defaults.

Updated: 2025-09-26
Type: How‑to

Baseline
- Bind: the service binds to `127.0.0.1` by default. Keep it private or put it behind a reverse proxy.
- Admin endpoints: set an admin token and require it on sensitive routes (the unified server now rejects admin requests unless `ARW_DEBUG=1` or a valid token is presented).
  - Env: `ARW_ADMIN_TOKEN=your-secret`
  - Header: `X-ARW-Admin: your-secret`
- Events & State: when `ARW_ADMIN_TOKEN` is set, `/events` and sensitive `/state/*` endpoints require the token. Keep these behind auth and a reverse proxy if exposed.
- Debug mode: unset `ARW_DEBUG` in production. With `ARW_DEBUG=1`, sensitive endpoints are open locally.
- Rate limits: adjust admin limiter, e.g. `ARW_ADMIN_RL="60/60"` (limit/window_secs).

Policy & Gating
- Immutable denies: edit [`configs/gating.toml`](https://github.com/t3hw00t/ARW/blob/main/configs/gating.toml) to add keys like `"tools:*"` or `"models:*"`.
- Contracts: add time‑bound denies with optional auto‑renew and subject filters.
- Ingress/Egress: use keys like `io:ingress:tools.<id>` and `io:egress:chat` to shape inputs/outputs.
 - Recommended production deny: block introspection endpoints: `deny_user = ["introspect:*"]`.

Capsules & Trust (RPU)
- Trust store: [`configs/trust_capsules.json`](https://github.com/t3hw00t/ARW/blob/main/configs/trust_capsules.json) lists allowed issuers and public keys.
- Generate keys (ed25519) and sign capsules:
  - `arw-cli capsule gen-ed25519` → save keys securely; put pubkey in `trust_capsules.json`.
  - `arw-cli capsule sign-ed25519 <sk_b64> capsule.json` → add `signature` to the capsule.
- Adoption: pass a verified capsule via `X-ARW-Capsule: <json>` header on admin‑authenticated requests.
- Legacy `X-ARW-Gate` headers are rejected (410); update any automation that still uses the retired name.
- Failure telemetry: legacy requests emit `policy.capsule.failed` and `policy.decision` events so monitoring catches rejected capsules.
- Alpha: see Architecture → Asimov Capsule Guard for current capsule refresh coverage and backlog roadmap.
- Env override: `ARW_TRUST_CAPSULES=/path/to/trust_capsules.json`.

Reverse Proxy
- Terminate TLS and IP-restrict at your proxy (Nginx, Caddy, Traefik) and forward to `127.0.0.1:8091` (unified server). Enable `ARW_DEBUG=1` only when you explicitly need `/admin/debug`.
- Set `ARW_DOCS_URL=https://your-domain/docs` so the debug UI can link to your public docs.
- Keep CORS strict; only enable `ARW_CORS_ANY=1` in development.

Nginx example
```
server {
  listen 443 ssl;
  server_name your-domain;

  # ssl_certificate /path/fullchain.pem;
  # ssl_certificate_key /path/privkey.pem;

  location / {
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_pass http://127.0.0.1:8091;
  }

  # (optional) restrict admin endpoints by path or IP here as a second guard
}
```

Caddy example
```
your-domain {
  encode zstd gzip
  reverse_proxy 127.0.0.1:8091
  @admin {
    path /admin/debug* /admin/memory* /admin/models* /admin/governor* /admin/introspect* /admin/feedback* /events* /admin/emit* /admin/shutdown
  }
  # optional: restrict @admin by client IPs
}
```

System Service
- Run as a non‑root user.
- Persist state/logs in a dedicated directory; set `ARW_STATE_DIR`, `ARW_LOGS_DIR` if needed.
- Use a supervisor you trust (systemd user service, launchd, NSSM/Task Scheduler on Windows).

Systemd (user) example
```
# ~/.config/systemd/user/arw.service
[Unit]
Description=ARW local service
After=network.target

[Service]
Environment=ARW_PORT=8091
Environment=ARW_DEBUG=0
Environment=ARW_ADMIN_TOKEN=change-me
Environment=ARW_HTTP_TIMEOUT_SECS=20
Environment=ARW_DOCS_URL=https://your-domain/docs
WorkingDirectory=%h/ARW
ExecStart=%h/ARW/target/release/arw-server
Restart=on-failure
RestartSec=2s

[Install]
WantedBy=default.target
```

Commands
```
systemctl --user daemon-reload
systemctl --user enable --now arw
journalctl --user -u arw -f
```

Clustering & Connectors
- Prefer NATS with authentication/TLS if exposing beyond localhost.
- Keep queue/bus set to `local` unless you need cross‑node communication.

Checklist
- [ ] `ARW_DEBUG` unset
- [ ] `ARW_ADMIN_TOKEN` set and required
- [ ] Patch safety enforcement enabled (`ARW_PATCH_SAFETY`) where risky patches must be rejected outright
- [ ] Admin rate‑limit tuned (`ARW_ADMIN_RL`)
- [ ] Gating policy in [`configs/gating.toml`](https://github.com/t3hw00t/ARW/blob/main/configs/gating.toml)
- [ ] Trust store configured; capsules signed and verified when used
- [ ] Reverse proxy/TLS if remote
- [ ] Logs/State directories scoped and monitored

## Security Headers & CSP
- Default response headers are set by middleware in the unified server:
  - `X-Content-Type-Options: nosniff`
  - `Referrer-Policy: no-referrer`
  - `X-Frame-Options: DENY`
  - `Permissions-Policy: geolocation=(), microphone=(), camera=()`
- CSP (Content Security Policy):
  - Auto-add for `text/html` unless disabled. Env: `ARW_CSP_AUTO=1` (default).
  - Presets: `ARW_CSP_PRESET=relaxed|strict` (default relaxed). Strict disables inline JS/CSS used by small pages.
  - Override: set `ARW_CSP` to a full policy string, or set to `off`/`0` to disable.
  - HSTS: enable `Strict-Transport-Security` with `ARW_HSTS=1` when serving over HTTPS.

## Access Logs (Optional)
- Enable structured access logs: `ARW_ACCESS_LOG=1` (target: `http.access`).
- Sampling: `ARW_ACCESS_SAMPLE_N` (default 1 = every request).
- Fields: method, path, matched route, status, `dt_ms`, client IP, `request_id`, request/response lengths.
- Optional fields:
  - User-Agent: `ARW_ACCESS_UA=1` (hash value with `ARW_ACCESS_UA_HASH=1`).
  - Referer: `ARW_ACCESS_REF=1` (strip query string with `ARW_ACCESS_REF_STRIP_QS=1`, default on).

### Rolling Access Log Files
- Enable rolling sink for `http.access`: `ARW_ACCESS_LOG_ROLL=1`.
- Directory: `ARW_ACCESS_LOG_DIR` (defaults to `${ARW_LOGS_DIR:-./logs}`), ensure writable.
- Prefix: `ARW_ACCESS_LOG_PREFIX` (default `http-access`).
- Rotation: `ARW_ACCESS_LOG_ROTATION=daily|hourly|minutely` (default `daily`).
- Example:
  ```bash
  ARW_ACCESS_LOG=1 \
  ARW_ACCESS_LOG_ROLL=1 \
  ARW_ACCESS_LOG_DIR=/var/log/arw \
  ARW_ACCESS_LOG_PREFIX=http-access \
  ARW_ACCESS_LOG_ROTATION=daily \
  arw-server
  ```

## Proxy Awareness
- By default, admin rate-limits use the remote socket address.
- When behind a trusted reverse proxy, set `ARW_TRUST_FORWARD_HEADERS=1` to honor `X-Forwarded-For`/`X-Real-IP`.

## Network Posture & Egress Firewall (Plan)
- Add a host‑local egress gateway (loopback proxy) plus a DNS guard for agent/tool traffic.
- Route containers and headless browsers through the proxy; block direct egress.
- Enforce allow/deny by domain/port without TLS MITM; record to an egress ledger.
- Postures: Off, Public only, Allowlist, Custom; leases provide temporary widenings.
- See: Architecture → Egress Firewall; Guide → Network Posture.

## Additional Lightweight Mitigations (Plan)
- Memory quarantine: retrieved content is untrusted until reviewed/scored; admit only with provenance + positive evidence score.
- Headless browsing: disable service workers and HTTP/3; same‑origin by default; extraction drops scripts/styles.
- Safe archives: extract in temp jails; canonicalize paths; cap size/time; limit nesting.
- Project isolation: per‑project caches/embeddings/indexes; no cross‑project mounts; “export views” are read‑only and revocable.
- Secrets: project vault only; redaction on snapshots and egress previews; periodic secret‑scan for artifacts.
- DNS guard: force local resolver; block raw UDP/53 and DoH/DoT from tools; rate‑limit lookups; alert on anomalies.
- Accelerator hygiene: zero VRAM/buffers; avoid persistence mode; prefer per‑job processes.
- Event integrity: mTLS, per‑episode nonces, monotonic sequence numbers; reject duplicates/out‑of‑order.
- Security posture: Relaxed/Standard/Strict presets per project.
