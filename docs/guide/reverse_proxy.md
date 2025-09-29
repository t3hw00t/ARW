---
title: Reverse Proxy
---

# Reverse Proxy

Run `arw-server` behind a TLS‑terminating reverse proxy for production exposure. These snippets demonstrate Caddy and NGINX with safe headers and timeouts.

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
- Consider a Web Application Firewall (WAF) and egress controls for defense‑in‑depth.

