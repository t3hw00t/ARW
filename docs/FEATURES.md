---
title: Features
---

# Features

Updated: 2025-09-10 (cluster/gating/hierarchy updates)

## Summary
- Open, local‑first agent runtime with versioned tool schemas and portable packaging. Rust core; optional plugins; thin UIs.

## Core Capabilities
- Interop: MCP (client/server), HTTP/WS with OpenAPI 3.1 + AsyncAPI. See: API and Schema (API_AND_SCHEMA.md).
- Observability: OpenTelemetry traces/logs/metrics; in‑process event bus (optional journal, SSE replay). See: Admin Endpoints (guide/admin_endpoints.md).
- Security & Policy: Central gating keys and deny contracts; ingress/egress guards; Policy Capsules; roadmap RPU for signatures/ABAC. See: Security Hardening (guide/security_hardening.md), Policy (POLICY.md).
- Extensibility: Static Rust plugins and dynamic WASI/WASM plugins; unified tool registry with JSON Schemas.
- Runtime & Memory: Orchestrator, pluggable Queue/Bus (local; NATS groups; JetStream planned), Run Capsules; layered memory and Memory Lab. See: Memory and Training (MEMORY_AND_TRAINING.md).
- Safety & Profiles: Sandboxed file/network allowlists; profiles (performance/balanced/power‑saver/custom); secrets and hints.
- Hardware & Performance: Hardware discovery; governor presets; model daemon + CAS for concurrent access. See: Hardware and Models (HARDWARE_AND_MODELS.md).
- Connections & Hierarchy: Connection registry, health checks, rate limits, QoS, tracing, policy; roles and negotiation (hello/offer/accept). See: Clustering (CLUSTERING.md), Hierarchy (HIERARCHY.md).

## Adapters
- llama.cpp (GGUF; CPU/GPU/NPU offload), ONNX Runtime (DirectML/CUDA/ROCm/OpenVINO/CoreML), OpenAI‑compatible HTTP. CPU fallback is mandatory.

## Companion Apps (Rust)
- Launcher (Tauri): tray, notifications, Events/Logs windows, Debug UI opener; optional autostart. See: Desktop Launcher (guide/launcher.md).
- Debug UI (Tauri): event stream, probe overlays, training console, logs/metrics.
- Model Manager (planned): browse/manage/convert/quantize; profiles & compatibility checks.
- Connection Manager (planned): discover, tune, and control connections/links with policy and health.

## Projects / Workstreams
- Core framework, Launcher, Debug UI, Model Manager, Connection Manager.

## See Also
- API and Schema (API_AND_SCHEMA.md)
- Security Hardening (guide/security_hardening.md)
- Deployment (DEPLOYMENT.md)
- Configuration (CONFIGURATION.md)
