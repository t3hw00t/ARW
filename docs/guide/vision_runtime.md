---
title: Vision Runtime Preview
---

# Vision Runtime Preview

Updated: 2025-10-09
Type: Guide (Preview)

This guide shows how to stage the preview vision runtimes (llava.cpp and Moondream) under the managed runtime supervisor. It focuses on a privacy-first, accessibility-aware setup that you can opt into today while the signed bundles are still in flight.

## Prerequisites
- `arw-server` built with the runtime supervisor enabled (`ARW_RUNTIME_SUPERVISOR=1`).
- NVIDIA GPU with CUDA ≥ 12.2 (preview bundle target); alternate accelerators arrive later.
- `configs/runtime/bundles.vision.json` present (ships with the repo as a placeholder catalog).
- `arw-cli` available for quick runtime inspections.

> **Preview status**  
> Artifact URLs in `bundles.vision.json` are placeholders until the signing pipeline lands. Use your own builds or private mirrors while we finalize distribution.

## Step 1 — Create a Runtime Manifest

```toml
# configs/runtime/runtimes.toml
version = 1

[[runtimes]]
id = "vision.llava.preview"
adapter = "process"
name = "LLaVA Vision Describe"
profile = "describe"
modalities = ["vision"]
accelerator = "gpu_cuda"
auto_start = false  # opt-in once you have health probes confirmed
preset = "balanced"
tags = { "bundle" = "llava.cpp-preview/linux-x86_64-gpu" }

[runtimes.process]
command = "/opt/llava/bin/llava-server"
args = [
  "--model", "/opt/llava/models/llava-v1.6-vicuna-q4.gguf",
  "--port", "12801",
  "--vision-device", "cuda:0",
  "--vision-warm-cache", "/var/lib/arw/runtime/llava/cache",
]
workdir = "/opt/llava"

[runtimes.process.env]
LLAVA_LOG_LEVEL = "info"

[runtimes.process.health]
url = "http://127.0.0.1:12801/healthz"
method = "GET"
expect_status = 200
timeout_ms = 3000
```

Tips:
- Set `auto_start = true` only after you confirm the health endpoint and GPU permissions.
- Point `--vision-warm-cache` at a directory on the same filesystem to let the supervisor reuse prompts.
- Use `runtime.preset` tags (for example `describe` vs `generate`) to steer future orchestration policies.
- `tags.bundle` helps operators map the runtime back to the bundle catalog entry exposed via `/state/runtime/bundles`.
- A ready-to-edit sample lives in `configs/runtime/runtimes.example.toml`.

## Step 2 — Register Consent & Provenance Policies
1. Create leases for `vision:capture` and `vision:describe` in your policy config (see Guardrail Gateway docs).
2. Configure Memory Fabric enrichment so every describe result writes into `memory.upsert` with provenance tags (`source.tool = "vision.describe"`).
3. Turn on the provenance ledger: `ARW_EGRESS_LEDGER_ENABLE=1`, even in local mode, to capture audit trails.

## Step 3 — Restart Supervisor & Verify

```bash
arw-cli runtime bundles reload   # rescan catalogs + manifests after editing
arw-cli runtime status           # shows descriptor + current state
arw-cli runtime restore vision.llava.preview --wait
```

Watch `/state/runtime_matrix` (SSE or `arw-cli runtime matrix`) for:
- `state = ready`
- Accessible status label (for example `Ready – Vision CUDA`)
- Restart budget counters (ensure they decrement on failures)

## Step 4 — Run the Vision Smoke Test (optional but recommended)

With the manifest in place you can exercise the automated supervisor check:

```bash
ARW_SERVER_BIN=target/debug/arw-server just runtime-smoke-vision
```

The helper spins up `arw-server` with the managed supervisor, launches a stub vision runtime via the manifest, verifies `/state/runtime_matrix`, runs a describe probe, and forces a restore to ensure the process restarts cleanly. By default it writes to `.smoke/vision/run.XXXX/` inside the repo so files stay stable even if `/tmp` is cleared; set `VISION_SMOKE_ROOT=/path/to/cache` if you want those runs to live elsewhere or persist between executions. Logs live under `./scripts/runtime_vision_smoke.sh` if you need to adapt it for real hardware.

> **Heads up** The script sets `ARW_SMOKE_MODE=vision`, which tells `arw-server` to skip non-essential background services (training, research watcher, distill, etc.). This keeps resource usage low while we only exercise the runtime supervisor.

## Accessibility Checklist
- Launcher overlays must expose keyboard focus, captions, and high-contrast outlines before enabling capture.
- Default describe output should include text alternatives for any generated imagery.
- Show a persistent indicator (and ARIA live region update) while the camera stream is active.

## Hardening & Next Steps
- Wire the same consent overlay components into audio once Step 3 is stable.
- Keep the dedicated vision smoke (`just runtime-smoke-vision`) in CI to catch regressions; extend it with GPU assertions when you wire real accelerators.
- When signed bundles land, replace your local binary path with the catalog artifact URL—no code changes required.
- If the smoke test ever aborts unexpectedly, terminate lingering stub processes with `pkill -f vision_stub.py` before re-running.

Related:
- [Managed Runtime Supervisor](../architecture/managed_runtime_supervisor.md)
- [Multi-Modal Runtime Plan](../architecture/multimodal_runtime_plan.md)
- [Guardrail Gateway](../architecture/egress_firewall.md)
