---
title: Multi-Modal Runtime Plan
---

# Multi-Modal Runtime Plan

Updated: 2025-10-07
Type: Blueprint (Proposed)
Status: Draft

## Overview

The managed runtime supervisor must cover text, audio, vision, and pointer/keyboard automation in a coherent, policy-aligned way. This blueprint breaks down the milestones that deliver voice generation, vision generation, and high-trust input control while keeping stability, accessibility, and future extensions in view.

## Objectives

- **Unified lifecycle**: all runtimes (text, audio, vision, pointer) register through the same adapter contract and expose status via `/state/runtimes`.
- **Vision-first rollout**: prioritize vision adapters and consent surfaces so camera capture, redaction, and provenance land before microphone tooling.
- **Generation & recognition parity**: each modality offers capture/ingest tools (recognition) and synthesis tools (generation) where feasible.
- **Consent-first UX**: mic/camera/pointer access always flows through leases, visible consent overlays, and revocation controls.
- **Accessibility first**: voice and pointer automation must respect screen readers, high-contrast modes, and alternative input devices.
- **Auditability**: every automation event (audio capture, image generation, pointer action) is journaled with provenance and optional replay.
- **Extensibility**: adding a new backend (e.g., new TTS engine) requires only adapter registration + bundle metadata.

## Modality Breakdown

### 1. Text / Core LLM
- Managed llama.cpp / ONNX Runtime / vLLM bundles (existing plan).
- Adapter features: context window, batching, accelerator hints, prompt cache warm-up.

### 2. Audio
- **Recognition**: Whisper.cpp (STT), optional DeepFilter for denoise.
- **Generation**: Piper or Coqui TTS for offline voice synthesis.
- **Tools**:
  - `/tools/audio.capture` → local recording; emits CAS blob + metadata.
  - `/tools/audio.transcribe` → uses Whisper.cpp adapter.
  - `/tools/audio.generate` → uses TTS adapter; supports voice presets, rate, emotion tags.
- **Bundles**: `configs/runtime/audio_bundles.json` covers CPU/GPU variants, sample rates, language support.
- **Accessibility**: expose captions transcript; ensure generated audio can default to system voice for clarity.

### 3. Vision
- **Recognition**: llava.cpp, Moondream, or other multimodal LLMs.
- **Generation**: optional text-to-image (e.g., Stable Diffusion runtimes) via adapter once licensing is settled.
- **Tools**:
  - `/tools/vision.capture` → still frame capture (camera) under lease.
  - `/tools/vision.describe` → image-to-text description with references.
  - `/tools/vision.generate` → text-to-image (if adapter supports).
- **Bundles**: `configs/runtime/vision_bundles.json` lists GPU/CPU requirements, VRAM minima; managed manifests live in `configs/runtime/runtimes.toml` during preview (`auto_start` toggles, adapter overrides).
- **Fallback**: when generation runtime absent, degrade gracefully to description or annotated placeholders.

### 4. Pointer & Keyboard
- **Adapters**: rely on OS-level automation libraries (e.g., enigo/tao for cross-platform pointer/key events) wrapped in sandboxed child processes.
- **Tools**:
  - `/tools/input.pointer` → move/click/scroll actions with bounding boxes.
  - `/tools/input.keyboard` → key sequences with rate limiting.
- **Safety**: high-trust leases, rate limits, “deadman switch” to pause automation, full replay log with timestamps.
- **Accessibility**: integrate with system accessibility APIs (VoiceOver/NVDA) to avoid conflicting gestures; default to highlight planned action before execution.

## Implementation Milestones

### Phase A – Foundations
1. Extend `RuntimeAdapter` contract definitions to include modality metadata (`text`, `audio`, `vision`, `pointer`).
2. Add `/state/runtimes` schema support for multi-modal status (current adapter, health, active leases).
3. Document bundle manifests for vision/audio adapters (`configs/runtime/vision_bundles.json`, `audio_bundles.json`).

### Phase B – Vision MVP
1. Package llava.cpp (vision) builds first; include consented describe pipelines before optional generation backends (Stable Diffusion, etc.).
2. Implement the vision adapter with capture/describe flows, redaction hooks, and recorder daemons; plug into runtime supervisor health, restart budgets, and Memory Fabric provenance journaling.
3. Ship Launcher “Vision” surface with camera consent overlays, keyboard-friendly focus controls, per-project retention settings, and quick describe actions.
4. Enforce `vision:capture`, `vision:describe`, `vision:generate` policy gates; publish manifest examples in `configs/runtime/runtimes.toml` with accessibility cues (contrast, captions) and per-adapter health probes.
5. Extend memory/tooling pipelines so extracted descriptions land in the Memory Fabric with provenance for replay and world-diff updates.

### Phase C – Audio MVP
1. Package Whisper.cpp + Piper binaries (CPU/GPU) with signed hashes.
2. Implement audio adapter with capture/transcribe/generate commands; reuse consent overlays and journal hooks from vision.
3. Update Launcher with Voice tab: microphone permission flow, level meters, caption preview, quick TTS playback, and fallbacks for muted devices.
4. Add policy gates `audio:capture`, `audio:transcribe`, `audio:generate`; document manifests mirroring the vision bundle format for harmonized ops.

### Phase D – Pointer Automation
1. Build pointer/keyboard adapter using sandboxed automation process with explicit allowlists.
2. Implement `input.pointer`/`input.keyboard` tools with rate limiting, bounding boxes, and event journaling.
3. Launcher Automation tab: big stop button, live feed of actions, config for maximum session duration.
4. Policy gates `input:pointer`, `input:keyboard`; require high-trust Autonomy Lane.

### Phase E – Stability & UX
1. Golden tests for each modality (capture + generate) with offline fixtures.
2. Accessibility smoke tests (screen reader compatibility, high-contrast, keyboard-only navigation).
3. Publish troubleshooting guides and onboarding docs.
4. Telemetry integration: `/metrics` counters for modality usage, failure rates.

## Documentation Updates
- `docs/architecture/managed_llamacpp_runtime.md`: reference this blueprint; expand multi-modal tooling sections.
- New tutorials: `guide/runtime_manager.md`, `guide/vision_runtime.md`, `guide/pointer_automation.md`.
- Update CLI docs with new `arw-cli` commands once adapters expose them.
- Add manifest appendix covering `auto_start` and adapter override fields.

## Initial Tasks
1. `t-multimodal-0001`: Extend runtime adapter trait + `/state/runtimes` schema for modality metadata.
2. `t-multimodal-0002`: Define vision bundle manifest and supervisor install pipeline; stage llava.cpp baselines.
3. `t-multimodal-0003`: Launcher consent overlay components (shared across vision/audio/pointer) with screen-reader focus tests.
4. `t-multimodal-0004`: Policy gating expansions (`vision:*`, `audio:*`, `input:*`) plus Memory Fabric provenance hooks for captured media.

## Open Questions
- Licensing/size constraints for bundling vision generation runtimes; may require optional download.
- GPU scheduling when multiple modalities contend (LLM + vision): need fairness heuristics.
- How soon to expose automation in public builds vs. pilot behind Autonomy Lane.

---
