---
title: Kubernetes (Helm)
---

# Kubernetes (Helm)

Updated: 2025-09-20
Type: How‑to

Deploy the unified ARW server on Kubernetes using the provided Helm chart.

!!! note "Legacy chart"
    Use the unified `arw-server` chart (port 8091). Legacy charts have been retired.

## Prerequisites
- Kubernetes cluster with an ingress controller (optional)
- Helm 3
- (Optional) GHCR auth if pulling a private image

## Install (Unified Server)

```bash
# From repo root (local chart)
helm upgrade --install arw deploy/charts/arw-server \
  --namespace arw --create-namespace \
  --set image.repository=ghcr.io/<owner>/arw-server \
  --set image.tag=latest \
  --set env.ARW_DEBUG=0 \
  --set env.ARW_BIND=0.0.0.0 \
  --set env.extra[0].name=ARW_ADMIN_TOKEN \
  --set env.extra[0].value=your-secret
```

## Pulling from GHCR (private)

```bash
# Create a registry secret for GHCR
kubectl create secret docker-registry ghcr \
  --docker-server=ghcr.io \
  --docker-username=<owner> \
  --docker-password='<ghcr-pat>' \
  --namespace arw

# Reference the secret in values
helm upgrade --install arw deploy/charts/arw-server \
  --namespace arw --create-namespace \
  --set image.repository=ghcr.io/<owner>/arw-server \
  --set image.tag=main \
  --set image.pullSecrets={ghcr} \
  --set env.ARW_DEBUG=0 \
  --set env.ARW_BIND=0.0.0.0 \
  --set env.extra[0].name=ARW_ADMIN_TOKEN \
  --set env.extra[0].valueFrom.secretKeyRef.name=arw-admin \
  --set env.extra[0].valueFrom.secretKeyRef.key=token
```

Alternatively, template and review:

```bash
helm template arw deploy/charts/arw-server --namespace arw | less
```

## Ingress (optional)

```bash
helm upgrade --install arw deploy/charts/arw-server \
  --namespace arw --create-namespace \
  --set ingress.enabled=true \
  --set ingress.className=traefik \
  --set ingress.hosts[0].host=arw.example.com \
  --set ingress.hosts[0].paths[0].path=/ \
  --set ingress.hosts[0].paths[0].pathType=Prefix

## Rolling Access Logs

Enable structured access logs and rolling files:

```bash
helm upgrade --install arw deploy/charts/arw-server \
  --namespace arw --create-namespace \
  --set env.ARW_ACCESS_LOG=1 \
  --set env.ARW_ACCESS_SAMPLE_N=1 \
  --set env.ARW_ACCESS_LOG_ROLL=1 \
  --set env.ARW_ACCESS_LOG_DIR=/var/log/arw \
  --set env.ARW_ACCESS_LOG_PREFIX=http-access \
  --set env.ARW_ACCESS_LOG_ROTATION=daily
```

Optional extra fields: add `--set env.ARW_ACCESS_UA=1 --set env.ARW_ACCESS_UA_HASH=1 --set env.ARW_ACCESS_REF=1`.
```

## Verify

```bash
kubectl -n arw get pods
kubectl -n arw port-forward deploy/arw-server 8091:8091 &
curl -sS http://127.0.0.1:8091/healthz
```

## Security Notes
- Set a strong `ARW_ADMIN_TOKEN` via secret.
- Keep `ARW_DEBUG=0` in clusters.
- Use NetworkPolicies and appropriate ingress settings as needed.
