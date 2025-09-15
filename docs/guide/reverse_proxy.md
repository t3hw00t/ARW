---
title: Reverse Proxy (Caddy/Traefik)
---

# Reverse Proxy (Caddy/Traefik)

Updated: 2025-09-14
Type: How‑to

Terminate TLS and proxy to ARW running on `127.0.0.1:8090` or in Docker.

## Caddy

`Caddyfile`:

```
arw.example.com {
  encode zstd gzip
  reverse_proxy 127.0.0.1:8090
}
```

Run:

```bash
caddy run --config Caddyfile
```

Docker (compose snippet):

```yaml
services:
  arw-svc:
    image: ghcr.io/<owner>/arw-svc:latest
    environment:
      - ARW_BIND=0.0.0.0
      - ARW_PORT=8090
      - ARW_DEBUG=0
      - ARW_ADMIN_TOKEN=your-secret
      - ARW_TRUST_FORWARD_HEADERS=1
    networks: [web]
  caddy:
    image: caddy:2
    ports: ["80:80", "443:443"]
    volumes:
      - ./Caddyfile:/etc/caddy/Caddyfile:ro
      - caddy-data:/data
      - caddy-config:/config
    networks: [web]
volumes:
  caddy-data: {}
  caddy-config: {}
networks:
  web: {}
```

## Traefik

Static config (file provider example):

```yaml
http:
  routers:
    arw:
      rule: Host(`arw.example.com`)
      service: arw
      entryPoints: [ websecure ]
      tls: {}
  services:
    arw:
      loadBalancer:
        servers:
          - url: "http://127.0.0.1:8090"
```

Docker labels example:

```yaml
services:
  arw-svc:
    image: ghcr.io/<owner>/arw-svc:latest
    environment:
      - ARW_BIND=0.0.0.0
      - ARW_PORT=8090
      - ARW_DEBUG=0
      - ARW_ADMIN_TOKEN=your-secret
      - ARW_TRUST_FORWARD_HEADERS=1
    labels:
      - "traefik.enable=true"
      - "traefik.http.routers.arw.rule=Host(`arw.example.com`)"
      - "traefik.http.routers.arw.entrypoints=websecure"
      - "traefik.http.routers.arw.tls=true"
      - "traefik.http.services.arw.loadbalancer.server.port=8090"
```

## Security Notes
- Keep `ARW_DEBUG=0` behind proxies; require strong `ARW_ADMIN_TOKEN` for `/admin/*`.
- Add rate‑limiting and IP allowlists at the proxy where possible.
