---
title: Systemd Service
---

# Systemd Service

Updated: 2025-09-20
Type: How‑to

Run the unified ARW server (`arw-server`) as a user service, either natively or via Docker.

!!! note "Legacy"
    The unified `arw-server` (default port 8091) replaces the legacy bridge. All new deployments should run this service directly.

## Environment file

`/etc/default/arw-server` (root) or `~/.config/arw-server.env` (adjust `EnvironmentFile` accordingly):

```
ARW_PORT=8091
ARW_BIND=127.0.0.1
ARW_DEBUG=0
ARW_ADMIN_TOKEN=your-secret
ARW_IMAGE=ghcr.io/<owner>/arw-server:latest
```

## Native unit

Install binary at `/usr/local/bin/arw-server`, then:

```
sudo install -m644 ops/systemd/arw-server-native.service /etc/systemd/system/arw-server@.service
sudo systemctl daemon-reload
systemctl --user enable --now arw-server@$(whoami)
```

## Docker unit

```
sudo install -m644 ops/systemd/arw-server-container.service /etc/systemd/system/arw-server-container@.service
sudo systemctl daemon-reload
systemctl --user enable --now arw-server-container@$(whoami)
```

## Hardening

See [Systemd Overrides](systemd_overrides.md) for cgroup weights and additional restrictions. Consider running behind a TLS reverse proxy and keeping `ARW_BIND=127.0.0.1`.
