---
title: Hardware and Models
---

# Hardware and Models
{ .topic-trio style="--exp:.6; --complex:.7; --complicated:.6" data-exp=".6" data-complex=".7" data-complicated=".6" }

Updated: 2025-09-06.

See also: [Configuration](CONFIGURATION.md), [Glossary](GLOSSARY.md)

## Objectives

Robust accelerator access (CPU/GPU/NPU) through open stacks.

High-level performance vs. power presets with auto-tuning.

Safe, concurrent model/file access for multiple actors.

Model interoperability and automatic performance optimization.

Dedicated Model Manager app for model lifecycle.

## Hardware Capabilities (arw-hw)

Probe devices, RAM/VRAM/bandwidth, dtypes (fp32/16/bf16/int8/4), driver/runtime versions, features.

Emit normalized HwReport, publish to event bus, expose /hw/report + MCP tool hw.probe.

## Adapters

llama.cpp: GGUF models; configurable offload; KV cache strategies.

ONNX Runtime: bridges DirectML/CUDA/ROCm/OpenVINO/CoreML when present; CPU fallback.

Vendor shims optional (e.g., TensorRT*) if installed.

If any accelerator path fails, the runtime must degrade to CPU and emit FallbackApplied events.

## Governor & Presets (arw-governor)

Profiles: performance, balanced, power-saver, custom.

Controls: threads, batch, kv cache size/placement, offload %, speculative decoding, IO priority.

Signals: battery %, thermals, utilization; Actions: live reconfig, pool scaling.

Policy-bindable: e.g., deny high-power after 20:00.

Note: where supported by the OS, network/disk IO priorities also apply to connectors/links managed by the Connection Manager.

## Concurrency (arw-modeld + arw-cas)

arw-modeld: centralized model loading; pooling, batching, leasing, QoS hints; HTTP/IPC control.

arw-cas: content-addressable, mmapped read-only artifacts; atomic swaps; GC for unreferenced blobs.

## Interoperability (arw-model-manifest)

ModelManifest: name, version, license, arch, ctx window, tokenizer, formats (GGUF/ONNX/safetensors), quantizations, adapter compatibility, recommended presets.

Compatibility solver selects best runtime given hardware; proposes fallbacks.

## Model Manager App

Discover/download with checksums & resume; CAS import; manifest generation.

Convert/quantize helpers with estimated perf/quality deltas.

Apply profile (dry-run validation); detect conflicts and suggest alternatives.

Export/import bundles for portability.

## Auto-Tune (arw-autotune)

Bench representative tasks; search over threads/batch/kv/offload/quant/speculative decoding.

Persist tuned profiles per device/model under /configs/presets/*.toml.

## Safety & Fallbacks

Policy-gated hardware actions, timeouts/circuit-breakers.

Graceful degradation to next-best adapter; events emitted for visibility.

## APIs & Events

HTTP (admin): /admin/hw/report, /admin/models, /admin/models/{id}/manifest, /admin/governor/profile; POST /admin/models/{id}/load|apply-profile|convert|quantize; POST /admin/autotune/run

MCP tools: hw.probe, model.list, model.applyProfile, model.convert, model.load

Events: HwDetected, ModelLoaded, PoolScaled, AutotuneStarted/Finished, GovernorChanged, Throttle, FallbackApplied
