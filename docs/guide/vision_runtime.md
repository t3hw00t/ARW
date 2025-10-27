---
title: Vision Runtime Preview
---

# Vision Runtime Preview

Updated: 2025-10-27
Type: Guide (Preview)

This guide shows how to stage the preview vision runtimes (llava.cpp and Moondream) under the managed runtime supervisor. It focuses on a privacy-first, accessibility-aware setup that you can opt into today while the signed bundles are still in flight.

## Prerequisites
- `arw-server` built from the current main branch (the managed runtime supervisor ships enabled by default; no additional env flag needed).
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

## Step 4 - Run the Vision Smoke Test (optional but recommended)

With the manifest in place you can exercise the automated supervisor check:

```bash
just runtime-smoke-vision
```

The helper spins up `arw-server` with the managed supervisor, launches a stub vision runtime via the manifest, verifies `/state/runtime_matrix`, runs a describe probe, and forces a restore to ensure the process restarts cleanly. By default it writes to `.smoke/vision/run.XXXX/` inside the repo so files stay stable even if `/tmp` is cleared; set `VISION_SMOKE_ROOT=/path/to/cache` if you want those runs to live elsewhere or persist between executions. If you already have a custom server build, export `ARW_SERVER_BIN=/path/to/arw-server` before running the command; otherwise the helper builds `arw-server` automatically. Logs live under `./scripts/runtime_vision_smoke.sh` if you need to adapt it for real hardware.

> **Heads up** The script sets `ARW_SMOKE_MODE=vision`, which tells `arw-server` to skip non-essential background services (training, research watcher, distill, etc.) and keeps the heavy read-model sweep disabled. Only the runtime matrix and light telemetry polls stay active, so the stub run remains friendly to low-memory dev kits.

On lower-memory machines, export `VISION_CARGO_JOBS=1` before running the smoke to force a single-job `cargo build`. The helper streams build output into `.smoke/vision/run.XXXX/cargo-build.log`, and it reuses the compiled binary on subsequent runs.

Need to inspect the generated manifest or logs after the run? Set `VISION_SMOKE_KEEP_TMP=1` to keep the temporary directory instead of cleaning it up automatically.

The helper now fails fast if `/state/runtime_matrix` is missing accessible strings (`label`, `severity_label`, `aria_hint`, descriptive `detail` rows) or ships a non-positive `ttl_seconds`, catching regressions before they land in CI dashboards.

Runtime smoke mode also bypasses the state observer bus subscriber, so event fan-out does not accumulate while the stub runtime is cycling. If you do hit slow networking (for example in nested containers), tweak the health/describe probes via `VISION_CURL_MAX_TIME`, `VISION_CURL_CONNECT_TIMEOUT`, and the retry knobs (`VISION_CURL_RETRY`, `VISION_CURL_RETRY_DELAY`); the script now applies them across every `curl` call.

Repeated local runs drop their artifacts under `.smoke/vision/`; the helper now auto-prunes older directories (keeping the latest six or any runs newer than a week). Override the knobs with `VISION_SMOKE_KEEP_RECENT`, `VISION_SMOKE_RETENTION_SECS`, or disable pruning entirely via `VISION_SMOKE_DISABLE_PRUNE=1`. Touch `.keep` inside a run directory if you want to pin it between runs.

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

## OCR Compression (Lite, feature-gated)

The OCR toolchain supports a lightweight, pre‑OCR image pipeline and an optional external “vision compression” backend. Build flags and environment knobs control behavior:

- Build features (server):
  - `arw-server/ocr_tesseract` enables the legacy Tesseract backend.
  - `arw-server/ocr_compression` enables the vision compression backend and related wiring.
- Backend selection (env):
  - `ARW_OCR_BACKEND` or `ARW_VISION_BACKEND`: `legacy` or `vision_compression`.
  - `ARW_OCR_QUALITY` or `ARW_VISION_QUALITY`: `lite`, `balanced`, `full`.
- External backend endpoint (when `ocr_compression` is built):
  - `ARW_OCR_COMPRESSION_ENDPOINT` (required), e.g. `http://127.0.0.1:18081/ocr`.
  - `ARW_OCR_COMPRESSION_TIMEOUT_SECS` (default 120).

Lite pre‑OCR steps (always safe on CPU):
- Grayscale conversion; downscale so max dimension ≤ 1280 px.
- Captured in sidecar metadata (`preprocess_steps`) and counters.

Metrics (Prometheus):
- `arw_ocr_preprocess_total{quality}` — pre‑OCR transformations applied.
- `arw_ocr_preprocess_ms{quality}` — pre‑OCR latency in milliseconds.
- `arw_ocr_preprocess_scale_ratio{quality}` — geometric area ratio (after/before).
- `arw_ocr_preprocess_size_ratio{quality}` — file size ratio (after/before) for the prepared image.
- `arw_ocr_backend_fallbacks_total{from,to}` — vision → legacy fallbacks.
- `arw_ocr_cache_hits_total{backend,quality}` — cache reuse hits.
- `arw_ocr_runs_total{backend,quality,runtime}` — executed OCR runs.

Notes:
- If `ocr_compression` isn’t compiled, the vision backend reports “unavailable” and the tool falls back to legacy when requested.
- Low‑spec devices can set `ARW_PREFER_LOW_POWER=1` (or use the Eco preset) to steer quality to `lite` by default.
