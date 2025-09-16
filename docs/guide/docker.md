---
title: Docker
---

# Docker

Updated: 2025-09-15
Type: How‑to

Run the ARW service in a container. This guide covers local build/run, docker compose, and pulling prebuilt images from GHCR.

## Images

- Registry: `ghcr.io/<owner>/arw-svc`
- Tags: `main`, `vX.Y.Z`, `latest` (on release), and `sha-<shortsha>`

## Local Build & Run

```bash
# Build (from repo root)
docker build -f apps/arw-svc/Dockerfile -t arw-svc:dev .

# Run (bind externally for host access)
docker run --rm -p 8090:8090 \
  -e ARW_BIND=0.0.0.0 \
  -e ARW_PORT=8090 \
  -e ARW_DEBUG=1 \
  -e ARW_ADMIN_TOKEN=dev-admin \
  arw-svc:dev

# Verify
curl -sS http://127.0.0.1:8090/healthz
```

## Docker Compose

```bash
# Use provided compose file (reads .env if present)
docker compose up --build -d

# Verify
curl -sS http://127.0.0.1:8090/healthz
```

### Rolling Access Logs

Write structured access logs to rotating files in Docker/Compose:

```bash
docker run --rm -p 8090:8090 \
  -e ARW_BIND=0.0.0.0 -e ARW_PORT=8090 \
  -e ARW_ACCESS_LOG=1 -e ARW_ACCESS_SAMPLE_N=1 \
  -e ARW_ACCESS_LOG_ROLL=1 \
  -e ARW_ACCESS_LOG_DIR=/var/log/arw \
  -e ARW_ACCESS_LOG_PREFIX=http-access \
  -e ARW_ACCESS_LOG_ROTATION=daily \
  -v $(pwd)/logs:/var/log/arw \
  arw-svc:dev
```

Optional fields: add `ARW_ACCESS_UA=1 ARW_ACCESS_UA_HASH=1 ARW_ACCESS_REF=1`.

Set in `.env` (see `.env.example`):
- `ARW_PORT=8090`
- `ARW_BIND=0.0.0.0` (or `127.0.0.1` when behind a reverse proxy)
- `ARW_DEBUG=0` (set `1` only for development)
- `ARW_ADMIN_TOKEN=<your-secret>`

## Pull from GHCR

```bash
# Replace <owner> with repo owner (e.g., t3hw00t)
IMG=ghcr.io/<owner>/arw-svc:latest

docker pull "$IMG"
docker run --rm -p 8090:8090 \
  -e ARW_BIND=0.0.0.0 -e ARW_PORT=8090 \
  -e ARW_DEBUG=0 -e ARW_ADMIN_TOKEN=your-secret \
  "$IMG"
```

## Health & Admin

```bash
curl -sS http://127.0.0.1:8090/healthz
curl -sS -H "Authorization: Bearer your-secret" http://127.0.0.1:8090/admin/probe
```

## Security Notes

- Keep `ARW_DEBUG=0` for production; set a strong `ARW_ADMIN_TOKEN`.
- Bind to `127.0.0.1` and put behind a TLS reverse proxy where possible.
- Container includes a healthcheck; compose and Kubernetes examples also include readiness probes.
