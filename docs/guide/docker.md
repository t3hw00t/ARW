---
title: Docker
---

# Docker

Updated: 2025-09-20
Type: How‑to

Run the unified ARW server in a container. This guide covers local build/run, Docker Compose, and pulling prebuilt images from GHCR.

## Images

- Registry (unified server): `ghcr.io/<owner>/arw-server`
- Tags: `main`, `vX.Y.Z`, `latest` (on release), and `sha-<shortsha>`

## Local Build & Run (Unified Server)

```bash
# Build (from repo root)
docker build -f apps/arw-server/Dockerfile -t arw-server:dev .

# Run (headless unified server on 8091)
docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 \
  -e ARW_PORT=8091 \
  -e ARW_ADMIN_TOKEN=dev-admin \
  arw-server:dev

# Verify
curl -sS http://127.0.0.1:8091/healthz
curl -sS http://127.0.0.1:8091/about | jq
```

## Docker Compose

```bash
# Use provided compose file (reads .env if present)
docker compose up --build -d

# Verify unified server health
curl -sS http://127.0.0.1:8091/healthz
```

The compose file defaults to the unified server on port 8091 and binds to `127.0.0.1` by default. To expose it externally, override `ARW_BIND=0.0.0.0` and set a strong `ARW_ADMIN_TOKEN`. The server refuses to start on a public bind without a token to prevent accidental exposure.

## Rolling Access Logs

Write structured access logs to rotating files with the unified server container:

```bash
docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 -e ARW_PORT=8091 \
  -e ARW_ACCESS_LOG=1 -e ARW_ACCESS_SAMPLE_N=1 \
  -e ARW_ACCESS_LOG_ROLL=1 \
  -e ARW_ACCESS_LOG_DIR=/var/log/arw \
  -e ARW_ACCESS_LOG_PREFIX=http-access \
  -e ARW_ACCESS_LOG_ROTATION=daily \
  -v $(pwd)/logs:/var/log/arw \
  arw-server:dev
```

Optional fields: add `ARW_ACCESS_UA=1 ARW_ACCESS_UA_HASH=1 ARW_ACCESS_REF=1`.

Set in `.env` (see `.env.example`):
- `ARW_PORT=8091`
- `ARW_BIND=0.0.0.0` (or `127.0.0.1` when behind a reverse proxy)
- `ARW_DEBUG=0` (set `1` only when you intentionally expose the debug UI)
- `ARW_ADMIN_TOKEN=<your-secret>`

## Pull from GHCR

```bash
# Replace <owner> with repo owner (e.g., t3hw00t)
IMG=ghcr.io/<owner>/arw-server:latest

docker pull "$IMG"
docker run --rm -p 8091:8091 \
  -e ARW_BIND=0.0.0.0 -e ARW_PORT=8091 \
  -e ARW_ADMIN_TOKEN=your-secret \
  "$IMG"
```

Legacy image:
```bash
```

## Security Notes

- Keep `ARW_DEBUG=0` for production; set a strong `ARW_ADMIN_TOKEN`.
- Bind to `127.0.0.1` and front with TLS whenever the container is reachable beyond localhost.
- Enable the egress ledger (`ARW_EGRESS_LEDGER_ENABLE=1`) and DNS guard (`ARW_DNS_GUARD_ENABLE=1`) when handling outbound HTTP.
- Restrict `/events` exposure; it streams action telemetry in real time.
