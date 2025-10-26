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

