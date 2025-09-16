---
title: Features
---

# Features

Updated: 2025-09-16
Type: Explanation

## Outcomes
- Turn messy folders, PDFs, and links into clean briefs, reports, or knowledge bases.
- Run focused research sprints: collect sources, extract facts, compare viewpoints, and draft with citations.
- Watch sites or docs for changes and get short, actionable updates.
- Turn vague goals into concrete plans, tasks, and next steps.
- Chat to explore data and export both answers and evidence.

## Why It’s Different
- Local‑first and privacy‑first by default; you decide access with time‑limited permission leases.
- Unified object graph with a single live event stream (SSE); every surface looks at the same state.
- Inspectable and replayable runs with sources, steps, tools used, and cost.
- Configurable strategies via Logic Units you can A/B, apply, and roll back without code changes.

## Scaling & Sharing (Opt‑In)
- Pool compute to your own GPU box or a trusted collaborator’s machine; offload under your rules and budget.
- Live co‑drive sessions; risky actions wait in a staging area for approval.
- Clear boundaries: egress previews and a ledger of what left and why.
- Fair splits across GPU time, tokens, and tasks.

## Under the Hood
- Open, local‑first agent runtime with versioned tool schemas and portable packaging. Rust core; optional plugins; thin UIs.

### Core Capabilities
- Interop: MCP (client/server), HTTP/WS with OpenAPI 3.1 + AsyncAPI. See: API and Schema (API_AND_SCHEMA.md).
- Observability: OpenTelemetry traces/logs/metrics; in‑process event bus (optional journal, SSE replay). See: Admin Endpoints (guide/admin_endpoints.md).
- Caching layers: Action Cache with CAS and singleflight; digest‑addressed blob serving with strong validators; read‑models over SSE (JSON Patch deltas with coalescing); llama.cpp prompt caching. See: Architecture → Caching Layers.
- Security & Policy: Central gating keys and deny contracts; ingress/egress guards; Policy Capsules; roadmap RPU for signatures/ABAC. See: [Security Hardening](guide/security_hardening.md), [Policy & Permissions](guide/policy_permissions.md).
 - Self‑improvement loop: goldens + rewards, A/B runner (with shadow), config patch engine, policy‑aware tuner, calibrated self‑model, nightly distillation, and a persisted experiments scoreboard + winners. See: Experiments (guide/experiments_ab.md).
- Egress control (planned): policy‑backed, per‑node egress gateway + DNS guard with project‑level network posture and an egress ledger. See: Architecture → Egress Firewall; Guide → Network Posture.
 - Lightweight mitigations (planned): memory quarantine; project isolation; belief‑diff review; cluster manifest pinning; hardened headless browsing; safe archive handling; DNS anomaly guard; accelerator zeroing; event sequencing; context rehydration check. See: Architecture → Lightweight Mitigations.
 - Models: enhanced downloader with resume, checksum, EWMA admission, disk reserve checks, content‑disposition filenames, cross‑platform finalize, and a simple concurrency limiter (`ARW_MODELS_MAX_CONC`).
- Extensibility: Static Rust plugins and dynamic WASI/WASM plugins; unified tool registry with JSON Schemas.
- Runtime & Memory: Orchestrator, pluggable Queue/Bus (local; NATS groups; JetStream planned), Run Capsules; layered memory and Memory Lab. See: Memory and Training (MEMORY_AND_TRAINING.md).
- Safety & Profiles: Sandboxed file/network allowlists; profiles (performance/balanced/power‑saver/custom); secrets and hints.
- Hardware & Performance: Hardware discovery; governor presets; model daemon + CAS for concurrent access. See: [Models Download](guide/models_download.md).
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
- Universal Feature Catalog (reference/feature_catalog.md)
