---
title: Reverse Proxy
---

# Reverse Proxy
Updated: 2025-09-29

Run `arw-server` behind a TLS-terminating reverse proxy for production exposure. Use the helper scripts for quick starts, or reference the snippets below when integrating with existing infrastructure.

## Helper Scripts

- `just proxy-caddy-generate host='arw.example.com' email='ops@example.com'` — emits `configs/reverse_proxy/caddy/Caddyfile.arw.example.com` with Let's Encrypt HTTP-01 enabled. Combine with `just proxy-caddy-start host='arw.example.com'` to launch Caddy (requires root when binding to ports 80/443) and `just proxy-caddy-stop` to shut it down. Pass `tls_module='cloudflare'` (or any Caddy DNS module) to swap ACME DNS-01.
- `just proxy-nginx-generate host='arw.example.com' cert='/etc/letsencrypt/live/arw/fullchain.pem' key='/etc/letsencrypt/live/arw/privkey.pem'` — writes `configs/reverse_proxy/nginx/arw.example.com/arw.conf`. Launch with `just proxy-nginx-start host='arw.example.com'` and stop with `just proxy-nginx-stop`.
- The Just recipes are thin wrappers over `scripts/reverse_proxy.sh`; call it directly when you need additional flags or want to run outside Just.

Generated assets live under `configs/reverse_proxy/` (ignored by git). Pair Caddy with the [Dev TLS profiles](dev_tls.md) helper for local certificates, or point NGINX at certs issued by your automation.

## Caddy

```caddyfile
arw.example.com {
  encode zstd gzip
  reverse_proxy 127.0.0.1:8091 {
    header_up X-Forwarded-For {remote_host}
    header_up X-Forwarded-Proto {scheme}
    header_up X-Forwarded-Host {host}
  }
}
```

Server env:
- `ARW_BIND=127.0.0.1` (default)
- `ARW_TRUST_FORWARD_HEADERS=1`

## NGINX

```nginx
server {
  listen 443 ssl http2;
  server_name arw.example.com;

  # TLS config omitted for brevity

  location / {
    proxy_http_version 1.1;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-Proto $scheme;
    proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
    proxy_set_header X-Real-IP $remote_addr;

    # SSE: disable buffering for event streams
    proxy_buffering off;

    proxy_pass http://127.0.0.1:8091;
  }
}
```

Server env:
- `ARW_BIND=127.0.0.1`
- `ARW_TRUST_FORWARD_HEADERS=1`

## Notes

- Keep admin UIs gated: set `ARW_ADMIN_TOKEN` and do not expose them publicly without auth.
- Enable HSTS at the proxy when serving over HTTPS in production.
- Consider a Web Application Firewall (WAF) and egress controls for defense-in-depth.
- For local testing, generate mkcert/self-signed assets via [Dev TLS Profiles](dev_tls.md).
