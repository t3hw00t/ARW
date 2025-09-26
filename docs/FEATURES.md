---
title: Features
---

# Features

Updated: 2025-09-20
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
- Stay local-first by default; federation and co-drive remain disabled until you enable them.
- **Preview** Pool compute to your own GPU box or a trusted collaborator’s machine; offload under your rules and budget.
- **Preview** Live co‑drive sessions keep risky actions in a staging area that waits for approval.
- **Preview** Clear boundaries arrive when you enable the Guardrail Gateway proxy + ledger to record what left and why.
- **Future** Fair splits across GPU time, tokens, and tasks.

> **Enable preview features** Add `[cluster]` → `enabled = true` to a config file the server already loads (e.g., `configs/default.toml`). If you keep overrides elsewhere, point `ARW_CONFIG` or `ARW_CONFIG_DIR` at that path. Then export `ARW_EGRESS_PROXY_ENABLE=1` and `ARW_EGRESS_LEDGER_ENABLE=1` to capture previews.

## Under the Hood
- Open, local‑first agent runtime with versioned tool schemas and portable packaging. Rust core; optional plugins; thin UIs.

### Core Capabilities
- Interop: MCP (client/server), HTTP/WS with OpenAPI 3.1 + AsyncAPI. See: API and Schema (API_AND_SCHEMA.md).
- Observability: OpenTelemetry traces/logs/metrics; in-process event bus (optional journal, SSE replay). See: Admin Endpoints (guide/admin_endpoints.md).
- Caching layers: Action Cache with CAS and singleflight; digest-addressed blob serving with strong validators; read-models over SSE (JSON Patch deltas with coalescing); llama.cpp prompt caching. See: Architecture → Caching Layers.
- Security & Policy: Central gating keys and deny contracts; ingress/egress guards; Policy Capsules; roadmap RPU for signatures/ABAC. See: [Security Hardening](guide/security_hardening.md), [Policy & Permissions](guide/policy_permissions.md).
- Visual capture: Agents and UIs call `ui.screenshot.capture` for screen/window/region snapshots with Activity lane previews, gallery management, annotation tooling, and optional OCR. See: [Screenshots](guide/screenshots.md).
- Managed runtimes: download, activate, and monitor llama.cpp/ONNX Runtime/vLLM bundles with automatic accelerator detection (CUDA/ROCm/DirectML/Metal/CoreML/Vulkan) and prompt cache warm-up. See: [Managed llama.cpp Runtime](architecture/managed_llamacpp_runtime.md).
- Multi-modal adapters: voice (Whisper.cpp, local TTS) and vision (llava.cpp, Moondream) share the same runtime lifecycle, with consent-first mic/camera leases and provenance events, plus optional pointer/keyboard automation under strict leases. See: Architecture → Managed llama.cpp Runtime → Multi-Modal Hooks.
- Self-improvement loop: goldens + rewards, A/B runner (with shadow), config patch engine, policy-aware tuner, calibrated self-model, nightly distillation (and on-demand via `POST /admin/distill`), and a persisted experiments scoreboard + winners. See: Experiments (guide/experiments_ab.md).
- Egress control (preview): enable the Guardrail Gateway for a policy-backed loopback proxy, DNS guard, project-level network posture, and an egress ledger. See: Architecture → Egress Firewall; Guide → Network Posture.
- Lightweight mitigations (planned): memory quarantine; project isolation; belief-diff review; cluster manifest pinning; hardened headless browsing; safe archive handling; DNS anomaly guard; accelerator zeroing; event sequencing; context rehydration check. See: Architecture → Lightweight Mitigations.
- Models: enhanced downloader with resume, checksum, EWMA admission, disk reserve checks, content-disposition filenames, cross-platform finalize, and a simple concurrency limiter (`ARW_MODELS_MAX_CONC`).
- Safety & Profiles: Sandboxed file/network allowlists; profiles (performance/balanced/power-saver/custom); secrets and hints.
- Hardware & Performance: Hardware discovery; governor presets; model daemon + CAS for concurrent access. See: [Models Download](guide/models_download.md).
- Connections & Hierarchy: Connection registry, health checks, rate limits, QoS, tracing, policy; roles and negotiation (hello/offer/accept). See: Federated Clustering (architecture/cluster_federation.md).

## Adapters
- llama.cpp (GGUF; CPU/GPU/NPU offload), ONNX Runtime (DirectML/CUDA/ROCm/OpenVINO/CoreML), OpenAI‑compatible HTTP. CPU fallback is mandatory.

## Companion Apps (Rust)
- Launcher (Tauri): tray, notifications, Events/Logs windows, Debug UI opener; optional autostart. See: Desktop Launcher (guide/launcher.md).
- Debug UI (Tauri): event stream, probe overlays, training console, logs/metrics.
- Model Manager: browse/manage/convert/quantize; profiles & compatibility checks (launcher window; see Guide → Models Download).
- Connection Manager: discover, tune, and control connections/links with policy and health (launcher window; see Guide → Connectors).

## Projects / Workstreams
- Core framework, Launcher, Debug UI, Model Manager, Connection Manager.

## See Also
- API and Schema (API_AND_SCHEMA.md)
- Security Hardening (guide/security_hardening.md)
- Deployment (guide/deployment.md)
- Configuration (CONFIGURATION.md)
- Universal Feature Catalog (reference/feature_catalog.md)
