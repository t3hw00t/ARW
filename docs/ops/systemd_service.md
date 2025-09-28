---
title: Systemd Service
---

# Systemd Service

Updated: 2025-09-21
Type: How‑to

Run the unified ARW server (`arw-server`) as a user service, either natively or via Docker.

!!! note "Unified surface"
    `arw-server` (default port 8091) is the supported runtime. The old bridge has been retired; all deployments should run this service directly.

## Environment file

`/etc/default/arw-server` (system-wide defaults) or `~/.config/arw-server.env` for per-user overrides. The unit files now ship with safe defaults, so only set the values you need to change.

```
ARW_PORT=8091
ARW_BIND=127.0.0.1
ARW_DEBUG=0
ARW_ADMIN_TOKEN=your-secret
ARW_IMAGE_OWNER=t3hw00t
ARW_IMAGE_TAG=latest
# Optional: uncomment to change where persistent state lives
# ARW_STATE_DIR=/var/lib/arw-server-demo/state

# Stability knobs (optional)
# ARW_SAFE_MODE_ON_CRASH=1
# ARW_SAFE_MODE_RECENT_MS=600000
# ARW_SAFE_MODE_MIN_COUNT=1
# ARW_SAFE_MODE_DEFER_SECS=30

# HTTP client tuning (optional)
# ARW_HTTP_TIMEOUT_SECS=20
# ARW_HTTP_CONNECT_TIMEOUT_SECS=3
# ARW_HTTP_TCP_KEEPALIVE_SECS=60
# ARW_HTTP_POOL_IDLE_SECS=90
```

## Native unit

Install the server binary at `/usr/local/bin/arw-server`, then install and enable the unit as root. The instance name maps to the Unix account that should run the service; the unit creates and confines its state under `/var/lib/arw-server-<user>/`.

```
sudo install -m644 ops/systemd/arw-server-native.service /etc/systemd/system/arw-server@.service
sudo systemctl daemon-reload
sudo systemctl enable --now arw-server@<unix-user>
```

## Docker unit

Install Docker, then:

```
sudo install -m644 ops/systemd/arw-server-container.service /etc/systemd/system/arw-server-container@.service
sudo systemctl daemon-reload
sudo systemctl enable --now arw-server-container@<unix-user>
```

## Hardening

Both units enable the sandbox features (`ProtectSystem=strict`, `NoNewPrivileges=yes`, `SystemCallFilter=@system-service`) and restrict write access to the dedicated state directory. See [Systemd Overrides](systemd_overrides.md) if you need different cgroup weights or additional hardening. In production we still recommend running behind a TLS reverse proxy and keeping `ARW_BIND=127.0.0.1` unless you are fronting the service with another firewall.
