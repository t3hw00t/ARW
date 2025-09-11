---
title: Security Hardening
---

# Security Hardening

This guide summarizes recommended steps to run ARW more securely beyond the local‑dev defaults.

Baseline
- Bind: the service binds to `127.0.0.1` by default. Keep it private or put it behind a reverse proxy.
- Admin endpoints: set an admin token and require it on sensitive routes.
  - Env: `ARW_ADMIN_TOKEN=your-secret`
  - Header: `X-ARW-Admin: your-secret`
- Debug mode: unset `ARW_DEBUG` in production. With `ARW_DEBUG=1`, sensitive endpoints are open locally.
- Rate limits: adjust admin limiter, e.g. `ARW_ADMIN_RL="60/60"` (limit/window_secs).

Policy & Gating
- Immutable denies: edit `configs/gating.toml` to add keys like `"tools:*"` or `"models:*"`.
- Contracts: add time‑bound denies with optional auto‑renew and subject filters.
- Ingress/Egress: use keys like `io:ingress:tools.<id>` and `io:egress:chat` to shape inputs/outputs.
 - Recommended production deny: block introspection endpoints: `deny_user = ["introspect:*"]`.

Capsules & Trust (RPU)
- Trust store: `configs/trust_capsules.json` lists allowed issuers and public keys.
- Generate keys (ed25519) and sign capsules:
  - `arw-cli capsule gen-ed25519` → save keys securely; put pubkey in `trust_capsules.json`.
  - `arw-cli capsule sign-ed25519 <sk_b64> capsule.json` → add `signature` to the capsule.
- Adoption: pass a verified capsule via `X-ARW-Gate: <json>` header on admin‑authenticated requests.
- Env override: `ARW_TRUST_CAPSULES=/path/to/trust_capsules.json`.

Reverse Proxy
- Terminate TLS and IP‑restrict at your proxy (Nginx, Caddy, Traefik) and forward to `127.0.0.1:8090`.
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
    proxy_pass http://127.0.0.1:8090;
  }

  # (optional) restrict admin endpoints by path or IP here as a second guard
}
```

Caddy example
```
your-domain {
  encode zstd gzip
  reverse_proxy 127.0.0.1:8090
  @admin {
    path /debug* /memory* /models* /governor* /introspect* /feedback*
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
Environment=ARW_PORT=8090
Environment=ARW_DEBUG=0
Environment=ARW_ADMIN_TOKEN=change-me
Environment=ARW_HTTP_TIMEOUT_SECS=20
Environment=ARW_DOCS_URL=https://your-domain/docs
WorkingDirectory=%h/Agent_Hub
ExecStart=%h/Agent_Hub/target/release/arw-svc
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
- [ ] Admin rate‑limit tuned (`ARW_ADMIN_RL`)
- [ ] Gating policy in `configs/gating.toml`
- [ ] Trust store configured; capsules signed and verified when used
- [ ] Reverse proxy/TLS if remote
- [ ] Logs/State directories scoped and monitored
