---
title: Runtime Matrix
---

# Runtime Matrix

Grid of Models × Hardware × Sandboxes with live health/throughput and load. Pin preferred combos per agent/project; surface degradations as events.

Matrix cells
- Model: id, size, quantization
- Hardware: CPU/GPU/NPU; memory/VRAM
- Sandbox: native|container|WASI; isolation level
- Health: ok/degraded/error; latency p50/p95; throughput; errors

Features
- Pin combos per agent/project; fallbacks with policy
- Event‑driven: `Runtime.Health`, `Models.Changed`, `Policy.*`
- Quick actions: (re)load model, switch runtime/sandbox, open logs

Backends
- Local engines (llama.cpp/Ollama/whisper/faster‑whisper)
- Optional remote fallback (opt‑in)

Policy
- Capabilities per cell (gpu, sandbox:<kind>) with TTL leases

