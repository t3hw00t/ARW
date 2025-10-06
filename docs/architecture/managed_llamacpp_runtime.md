---
title: Managed llama.cpp Runtime
---

# Managed llama.cpp Runtime

Updated: 2025-09-27
Type: Blueprint (Proposed)
Status: Draft (see [Managed Runtime Supervisor](managed_runtime_supervisor.md) for the cross-runtime plan and `spec/schemas/runtime_manifest.json` for manifest schema ties)

## Problem

Today ARW exposes a thin `ARW_LLAMA_URL` hook that assumes operators already know how to run llama.cpp, pick quantizations, and keep prompt caches warm. The experience is brittle:
- Users must install, configure, and supervise a separate daemon.
- Model downloads and runtime lifecycle are disconnected; Launcher cannot guarantee the model a user selected is actually live.
- Autonomous projects that rely on continuous inference have no way to express runtime requirements, health targets, or failover policies.

We need a first-class, flexible integration that feels native, stays policy-aware, and lays groundwork for additional backends (ONNX Runtime, vLLM, etc.).

## Goals

- **Zero-to-first-reply in minutes**: ship sane defaults so a new install can download a model, launch llama.cpp, and talk to it without editing env vars manually.
- **Dynamic personal control**: expose profiles (CPU-only, GPU-high-throughput, silent/offline, etc.), per-project model overrides, hot swapping, and schedule-based controls.
- **Accelerate everywhere**: detect and leverage GPUs/NPUs across Windows/macOS/Linux (CUDA, ROCm, Metal/MLX, DirectML, Vulkan, CoreML, Qualcomm HTP), falling back gracefully when a target is missing.
- **Autonomous-ready**: orchestrator and Autonomy Lane can express runtime requirements (model class, latency, min tokens/s), react to health events, and queue fallbacks automatically.
- **Extensible**: the same plumbing should let us plug in other local runtimes (ONNX Runtime, vLLM, llamafile) by implementing a shared adapter contract.
- **Observable + debuggable**: event stream surfaces load, warm-start, failures, and autoscaling hints; users can see why a runtime was restarted or denied.
- **Policy-aligned**: runtime processes inherit Guardrail settings (network posture, file scopes), honor leases/capsules, and log egress.

## Non-Goals

- Building a brand-new inference engine.
- Shipping proprietary model weights.

## Current State

- Model downloader (Model Steward) handles CAS storage and progress events (`apps/arw-server/src/models.rs`, `docs/guide/models_download.md`).
- Chat backend can call llama.cpp by `ARW_LLAMA_URL`, but there is no lifecycle control or health monitoring.
- Launcher UI advertises a Model Manager window yet depends on manual runtime setup.
- Orchestrator lacks explicit contracts for runtime capabilities; autonomous workflows lean on Autonomy Lane policy but no runtime orchestration.

## Proposed Architecture

### 1. Runtime Manager Service (Launcher + Server)

**Responsibilities**
- Discover, install, and update supported runtime binaries (start with llama.cpp prebuilt bundles per platform + optional GPU targets).
- Resolve model manifests from CAS, select quantization/rope settings, and generate runtime configs.
- Start/stop runtime processes under a supervisor with health checks, log capture, and crash restart budgets.
- Emit `runtime.state.changed` events (state, health, reason) and expose `/state/runtimes` read-model.

**Components**
- Launcher-side daemon controller (Rust + Tauri bridge) that shells out to managed runtime binaries.
- Server-side `RuntimeRegistry` resource that stores desired state (`desired`, `current`, `last_error`, `profiles`) in the kernel CAS.
- Config schema: `spec/schemas/runtime_profile.json` describing CPU/GPU/accelerator requirements, preferred quant, context window, batching, prompt cache path.
- Accelerator bundle catalog (`configs/runtime/bundles.json`) enumerating supported binaries/flags per platform (CUDA, ROCm, Metal/MLX, Vulkan, DirectML, CoreML, CPU-only) with signed hashes.

### 2. Model Catalog & Profiles

- Enrich model manifests (`spec/schemas/model_manifest.json`) with runtime metadata: hidden size, context window, quant flavors, recommended profile tags, hardware notes.
- Maintain curated presets in `configs/models/catalog.json` (e.g., "Llama 3.1 8B Q4") and map each to compatible accelerator bundles.
- Launcher UI surfaces cards with quick actions: Download, Activate, Benchmark, Set as default.
- Automatic suggestions: hardware probe (existing in `guide/compatibility.md`) feeds into profile selection and recommends the best accelerator path (e.g., DirectML vs. CUDA vs. Metal).

### 3. Configurability & Personalization

- **Profiles**: `performance`, `balanced`, `silent`, `custom`, plus accelerator-aware variants (`gpu-cuda`, `gpu-rocm`, `gpu-metal`, `npu-directml`, `npu-coreml`). Each maps to runtime flags (threads, context, GPU layers, batch size, kv cache strategy) and scheduler hints.
- **Per-project overrides**: store project-level desired model profile (`/projects/:id/runtime_profile`). UI toggles enable local adjustments without global disruption.
- **Schedules & triggers**: optional quiet hours, battery thresholds, or manual one-click “hibernate” state; tie into Launcher command palette.

### 4. Event & Task Automation for Autonomous Projects

- Define runtime requirement contract for orchestrator jobs: `RuntimeClaim { model_tag, min_tokens_per_sec, context_window, accelerator, features }`.
- Extend Autonomy Lane charter with runtime gates: lane can require `verified_model = true`, degrade to fallback models when primary fails health checks, or switch accelerators when thermal or power limits trigger.
- Add orchestrator tasks: `runtime.ensure_ready` (idempotent), `runtime.health_probe`, and `runtime.swap_model` to align with project plans.
- Introduce automation pipeline: when a project enters autonomous mode, orchestrator requests runtime claim; Runtime Manager ensures target model is downloaded, runtime warm, accelerator enabled, and prompt cache primed before job execution.
- Expose consistent events: `runtime.claim.acquired`, `runtime.claim.rejected`, `runtime.health.degraded`, `runtime.fallback.engaged`, `runtime.accelerator.switch`.

### 5. Extensibility Layer

- Define a `RuntimeAdapter` trait with capabilities: `prepare(model_manifest, profile)`, `launch()`, `shutdown()`, `health_report()`, `metrics()`, `supports(feature)`, `apply_patch(config_patch)`.
- Provide llama.cpp adapter first; design the trait so adapters can run as separate processes or in-process libraries.
- Document adapter handshake (`runtime_adapters.md`): registration metadata, binary packaging, sandbox requirements, accelerator capability descriptors.
- Roadmap: ONNX Runtime adapter using same contract (DirectML/ROCm/CUDA/OneDNN), vLLM adapter with GPU batching + PagedAttention, CoreML/Metal adapter for macOS/iOS-class hardware, Qualcomm/MediaTek NPU hooks via vendor SDKs.

### 6. Observability & Telemetry

- `/state/runtimes` read-model publishes: status, uptime, warm tokens, queue depth, last error, active model, profile, health grade.
- `/metrics` includes `runtime_tokens_per_sec`, `runtime_queue_wait_ms`, `runtime_restarts_total`.
- Launcher runtime panel shows sparkline + restart history, offers quick log download.
- Ties into Snappy Governor to enforce latency budgets via adaptive scheduling.

### 7. Security & Policy Integration

- Runtime processes run with least privilege: per-project temp directories, network posture from Guardrail Gateway (loopback-only unless explicitly allowed). GPU device access is scoped via platform-specific ACLs (CUDA_VISIBLE_DEVICES, Metal sandbox entitlements, DirectML device filters) to honor leases.
- Capsules: runtime adoption requires signed capsule acknowledging model hash and accelerator policy. Capsule Guard auto-refresh logs go through runtime events.
- Policy: new gating keys `runtime:manage`, `runtime:activate`, `runtime:override`, `runtime:accelerator` for admin surfaces; Autonomy Lane requires explicit grant.
- Per-accelerator security guidance doc (e.g., driver versions, isolation quirks) published alongside runtime bundles.

See also: [Multi-Modal Runtime Plan](multimodal_runtime_plan.md) for detailed milestones and bundle manifests.

### 8. Multi-Modal Runtime Hooks

- Ship sibling adapters for speech (local STT/TTS engines such as Whisper.cpp, DeepFilter, Piper) and vision (llava.cpp, llama.cpp vision builds, Moondream) that share the same lifecycle manager.
- Extend runtime profiles with modality flags (`text`, `vision`, `audio`), required peripherals (mic, camera), and policy prompts for consent.
- Provide unified capture/ingest services:
  - Audio: integrate with existing permission leases to gate mic access; expose `/tools/audio.capture`, `/tools/audio.transcribe`, and `/tools/audio.generate` wired to managed speech runtimes.
  - Visual: reuse screenshot/annotation pipeline and add camera capture tool gated by policy; vision runtimes consume captured frames via CAS and can emit `/tools/vision.describe` or `/tools/vision.generate` (image-to-text, text-to-image when adapters allow).
- Multi-modal inference events: publish `runtime.modality.started`, `runtime.modality.completed` with provenance and budgets.
- Launcher UI: add “Voice & Vision” tab showing per-device status, warm caches, and quick calibration flows.
- Autonomy Lane requirements: enforce explicit operator approval before enabling always-on audio/video capture; add automatic teardown when lane exits.
- Optional interactivity extensions: expose pointer/keyboard control as separate gated tools (`/tools/input.pointer`, `/tools/input.keyboard`) that require high-trust leases, replay logs, and explicit automation budgets.

## Implementation Plan

Implementation phases for text, audio, vision, and pointer automation now live in [Multi-Modal Runtime Plan](multimodal_runtime_plan.md#implementation-milestones). That document supersedes the rough phases listed here and should guide milestone and issue tracking.

## Required Updates & Touchpoints

- **Docs**: update `docs/guide/chat_backends.md`, `docs/guide/models_download.md`, add `guide/runtime_manager.md` tutorial, extend Autonomy Lane charter with runtime requirements, document accelerator compatibility matrix.
- **Config**: new `[runtime]` section in `configs/default.toml` with desired runtime defaults and accelerator preferences.
- **Events**: register new topics in `crates/arw-topics/src/lib.rs` (`runtime.state.changed`, `runtime.claim.*`, `runtime.accelerator.switch`).
- **Launcher**: new Tauri commands for runtime control, UI pane, hardware probe integration, driver/version warnings.
- **CI**: runtime smoke tests (start + health check) across CPU-only and accelerator builds; bundler packaging verification; optional hardware-in-loop tests where available.

## Open Questions

1. Do we bundle llama.cpp binaries directly or provide guided download (licensing, size)?
2. How aggressively do we auto-update runtimes, especially on air-gapped machines?
3. What is the fallback story when no GPU is available but user selects GPU profile (auto degrade or fail fast)?
4. Should prompt cache warm-up be optional per profile (battery impact)?
5. How do we expose adapter capability matrix in UI without overwhelming users?

## Appendix A – Event Flow (Happy Path)

1. User selects “Llama 3.1 8B Q4” profile in Launcher.
2. Model Manager ensures manifest + weights exist (downloads via Model Steward if missing).
3. Runtime Manager supervisor launches llama.cpp with generated config, posts `runtime.state.changed: starting`.
4. Health probe passes, state flips to `ready`; prompt cache warm-up seeds caches.
5. Orchestrator/project job issues `runtime.claim.request` for `project=alpha` → granted, `runtime.claim.acquired` emitted.
6. Chat/automation requests route to runtime via local gRPC/HTTP; Snappy Governor monitors latency.
7. If runtime crashes, supervisor restarts, `runtime.state.changed: restarting` emitted; orchestrator queues degrade events for pending jobs.

## Appendix B – Adapter Contract (Sketch)

```rust
pub trait RuntimeAdapter {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> CapabilitySet;
    async fn prepare(&self, ctx: PrepareContext) -> Result<PreparedRuntime, AdapterError>;
    async fn launch(&self, prepared: PreparedRuntime) -> Result<RuntimeHandle, AdapterError>;
    async fn shutdown(&self, handle: RuntimeHandle) -> Result<(), AdapterError>;
    async fn health(&self, handle: RuntimeHandle) -> Result<HealthReport, AdapterError>;
    async fn metrics(&self, handle: RuntimeHandle) -> Result<RuntimeMetrics, AdapterError>;
}
```

Adapters register themselves at startup; Runtime Manager chooses one based on manifest + profile.

---
