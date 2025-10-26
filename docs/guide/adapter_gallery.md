---
title: Adapter Gallery
---

# Adapter Gallery
Updated: 2025-10-26
Type: Reference

This page lists sample adapter manifests included with the repo and shows how to validate or smoke them locally.

## Samples

- llama-cpp (CPU): `adapters/llama-cpp-cpu.adapter.json`
  - Text inference via llama.cpp on CPU-only hosts
  - Modalities: text; Accelerator: cpu; No egress

- REST Bridge: `adapters/rest-bridge.adapter.json`
  - Proxy adapter bridging an upstream REST runtime
  - Modalities: text; Accelerator: other; Requires egress

- Community Host: `adapters/community-host.adapter.json`
  - Community-maintained host for experimental runtimes
  - Modalities: text; Accelerator: other; No egress by default

- Mock Adapter (demo): `adapters/mock.adapter.json`
  - Pairs with a tiny health server used by CI/local smoke
  - Modalities: text; Accelerator: cpu; Health endpoint `/healthz` on `http://127.0.0.1:8081`

## Validate

- Single file: `cargo run -p arw-cli -- adapters validate --manifest adapters/mock.adapter.json`
- Lint all: `bash scripts/lint_adapters.sh`
- Lint changed vs base: `BASE=origin/main bash scripts/lint_adapters_changed.sh`

## Smoke (local)

- One-shot (build mock server + health smoke):
  - Just: `just adapters-smoke-oneshot`
  - Mise: `mise run adapters:smoke:oneshot`

- Manual: run mock server, then probe via smoke:
  - `just adapters-mock-up` (or `mise run adapters:mock:up`)
  - `ADAPTER_SMOKE_HEALTH=1 bash scripts/adapter_smoke.sh`

See also: Adapter SDK guide for SDK usage and the smoke harness report format.

## Signing (optional)

You can sign a manifest with an RSA key using the helper scripts (not enforced by CI):

- Generate a key pair (example):
  - `openssl genrsa -out private.pem 2048`
  - `openssl rsa -in private.pem -pubout -out public.pem`

- Sign a manifest:
  - Just: `just adapters-sign manifest=adapters/mock.adapter.json key=private.pem`
  - Mise: `mise run adapters:sign MANIFEST=adapters/mock.adapter.json KEY=private.pem`
  - Produces `<manifest>.sig` (binary) and `<manifest>.sig.b64` (Base64).

- Verify a signature:
  - Just: `just adapters-verify manifest=adapters/mock.adapter.json pubkey=public.pem`
  - Mise: `mise run adapters:verify MANIFEST=adapters/mock.adapter.json PUBKEY=public.pem`
  - For Base64 signatures: pass `sig=<file.sig.b64>`.

Signing is an optional provenance aid for community galleries. Validation and smoke do not currently enforce signatures.
