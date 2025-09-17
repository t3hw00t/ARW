---
title: Systemd Service
---

# Systemd Service

Updated: 2025-09-16
Type: How‑to

Run ARW as a user service, either natively or via Docker.

!!! note "Legacy service"
    This guide targets the legacy `arw-svc` bridge, which listens on port 8090 and serves the classic debug UI. For the unified
    headless `arw-server` (default port 8091), use the Docker or service deployment docs under `guide/`.

## Environment file

`/etc/default/arw-svc` (root) or `~/.config/arw-svc.env` (adjust `EnvironmentFile` accordingly):

```
ARW_PORT=8090
ARW_BIND=127.0.0.1
ARW_DEBUG=0
ARW_ADMIN_TOKEN=your-secret
ARW_IMAGE=ghcr.io/<owner>/arw-svc:latest
```

## Native unit

Install binary at `/usr/local/bin/arw-svc`, then:

```
sudo install -m644 ops/systemd/arw-svc-native.service /etc/systemd/system/arw-svc@.service
sudo systemctl daemon-reload
systemctl --user enable --now arw-svc@$(whoami)
```

## Docker unit

```
sudo install -m644 ops/systemd/arw-svc-container.service /etc/systemd/system/arw-svc-container@.service
sudo systemctl daemon-reload
systemctl --user enable --now arw-svc-container@$(whoami)
```

## Hardening

See [Systemd Overrides](systemd_overrides.md) for cgroup weights and additional restrictions. Consider running behind a TLS reverse proxy and keeping `ARW_BIND=127.0.0.1`.
