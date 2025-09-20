---
title: Reverse Proxy (Caddy/Traefik)
---

# Reverse Proxy (Caddy/Traefik)

Updated: 2025-09-20
Type: How‑to

Terminate TLS and proxy to ARW running on `127.0.0.1:8091` (unified `arw-server`), or deploy via Docker.

## Caddy

Unified server (`arw-server`) on port 8091:

`Caddyfile`:

```
arw.example.com {
  encode zstd gzip
  reverse_proxy 127.0.0.1:8091
}
```

Run:

```bash
caddy run --config Caddyfile
```

Docker (compose snippet):

```yaml
services:
  arw-server:
    image: ghcr.io/<owner>/arw-server:latest
    environment:
      - ARW_BIND=0.0.0.0
      - ARW_PORT=8091
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
          - url: "http://127.0.0.1:8091"
```

Docker labels example:

```yaml
services:
  arw-server:
    image: ghcr.io/<owner>/arw-server:latest
    environment:
      - ARW_BIND=0.0.0.0
      - ARW_PORT=8091
      - ARW_DEBUG=0
      - ARW_ADMIN_TOKEN=your-secret
      - ARW_TRUST_FORWARD_HEADERS=1
    labels:
      - "traefik.enable=true"
      - "traefik.http.routers.arw.rule=Host(`arw.example.com`)"
      - "traefik.http.routers.arw.entrypoints=websecure"
      - "traefik.http.routers.arw.tls=true"
      - "traefik.http.services.arw.loadbalancer.server.port=8091"
```

## Security Notes
- Keep `ARW_DEBUG=0` for the unified server and require a strong `ARW_ADMIN_TOKEN`.
- Protect `/events`, `/actions`, and `/state/*` behind authentication and allowlists when exposing them through the proxy; `/events` streams live telemetry.
- Add rate‑limiting and IP allowlists at the proxy where possible.
