---
title: Runtime Quickstart (Non-Technical)
---

# Runtime Quickstart (Non-Technical)
Updated: 2025-10-16
Type: Tutorial

This walkthrough is aimed at operators who want to validate the managed runtime supervisor without digging into the codebase. It uses the automation we ship (`just` commands and helper scripts) so you can prepare weights and run the smoke test with minimal manual tinkering.

## Prerequisites

- Terminal access to the ARW workspace (this directory contains `Justfile`).
- A Hugging Face account and a “Read” access token. Create one at <https://huggingface.co/settings/tokens>.
- llama.cpp binaries compiled locally (see `docs/guide/runtime_matrix.md#building-llamacpp`). If you followed the standard instructions the binary lives at `cache/llama.cpp/build/bin/llama-server`.

!!! note
    Match the workspace mode to the shell you are using (e.g., `bash scripts/env/switch.sh windows-wsl` inside WSL) before running `just` commands. See [Environment Modes](../developer/environment_modes.md) for the full walkthrough.

## 1. Open the project shell

```bash
cd /path/to/ARW
```

If you are on Windows (WSL) or macOS, launch the terminal that already has access to the repo.

## 2. Run the guided check

The helper will:

1. Ask for your Hugging Face token (it’s only stored in-memory).
2. Download TinyLlama GGUF weights into `cache/models/`.
3. Locate the `llama-server` binary (auto-detects `cache/llama.cpp/build/bin/llama-server`; otherwise it will prompt for a path).
4. Launch the runtime smoke (stub by default, optional CPU/GPU stages when enabled).

```bash
just runtime-check
just runtime-check-weights-only  # download weights without running the smoke
```

Follow the on-screen prompts. If you are missing a compiled `llama-server` binary, you can stop here and revisit the build instructions later—the helper will fall back to simulated GPU mode so you can still verify the pipeline end-to-end.

## 3. Review the results

At the end of the run you will see:

- log locations (under `.smoke/runtime/run.*` by default, override with `RUNTIME_SMOKE_ROOT`),
- whether the CPU/GPU acceleration markers were detected (depending on which stages ran),
- any policy gating or restart-budget warnings emitted by the supervisor.

If the helper used the simulated mode (because no real binary or weights were available) you can rerun `just runtime-check` (or `just runtime-check-weights-only`) after resolving the prerequisites; cached weights are reused automatically. You can adjust the upstream sources in `configs/runtime/model_sources.json` if your organization mirrors weights internally.

> Tip: the CPU stage runs only when you export `RUNTIME_SMOKE_ALLOW_CPU=1`, and the GPU stage stays in simulated mode until you also set `RUNTIME_SMOKE_ALLOW_GPU=1`.

### Live Reload (manifests & bundles)
- While you iterate, runtime manifest changes are picked up automatically. Edit `configs/runtime/runtimes.toml` (or set `ARW_RUNTIME_MANIFEST` to point at a custom file) and the supervisor reloads definitions within a few seconds.
- Runtime bundle catalogs also auto-reload when files under `configs/runtime/*.json` or `<state>/runtime/bundles/` change. Inspect the current view at `/state/runtime/bundles` and the supervisor snapshot at `/state/runtime_supervisor`.

#### Watch changes live (SSE)
```bash
just sse-tail prefixes='service.health,state.read.model.patch' replay='10'
```
or:
```bash
curl -N -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  "http://127.0.0.1:8091/events?prefix=service.health,state.read.model.patch&replay=10"
```

#### Inspect watcher summary
```bash
curl -s -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  http://127.0.0.1:8091/state/runtime/watchers | jq
# Includes per-area status (ok/degraded), age since last reload/error, and an overall status.
```

#### Configure cooldown
- Default cooldown is 3 minutes; override via env or config.
- Env:
  ```bash
  export ARW_RUNTIME_WATCHER_COOLDOWN_MS=600000
  ```
- Config (`configs/default.toml`):
  ```toml
  [env]
  ARW_RUNTIME_WATCHER_COOLDOWN_MS = 600000
  ```

## Advanced options

- Download weights only:
  ```bash
  just runtime-weights
  ```
- Preview the smoke without executing anything:
  ```bash
  just runtime-smoke-dry-run
  ```
- Supply custom Hugging Face sources:
  ```bash
  LLAMA_MODEL_SOURCES="repo::file,repo2::file2" just runtime-weights
  ```
- Provide a checksum for any source by adding a `"checksum": "sha256:..."` entry in `configs/runtime/model_sources.json`; the helper validates downloads automatically when a checksum is present.
- Auto-download TinyLlama via the smoke helper (falls back to simulated GPU markers if the binary/weights are missing; export `RUNTIME_SMOKE_SKIP_AUTO_WEIGHTS=1` to stay offline):
  ```bash
  RUNTIME_SMOKE_ALLOW_CPU=1 \
  RUNTIME_SMOKE_GPU_POLICY=auto \
    RUNTIME_SMOKE_LLAMA_SERVER_BIN=/path/to/llama-server \
    RUNTIME_SMOKE_LLAMA_MODEL_PATH=/path/to/model.gguf \
    just runtime-smoke
  ```
- List configured mirrors without downloading anything:
  ```bash
  just runtime-mirrors

  # include -- --check to send a HEAD request to each mirror
  just runtime-mirrors -- --check
  ```
- Skip the guided flow and run the smoke directly (requires all env vars to be set):
  ```bash
  RUNTIME_SMOKE_ALLOW_CPU=1 \
  RUNTIME_SMOKE_CPU_POLICY=auto \
  RUNTIME_SMOKE_ALLOW_GPU=1 \
  RUNTIME_SMOKE_GPU_POLICY=require \
    RUNTIME_SMOKE_LLAMA_SERVER_BIN=/path/to/llama-server \
    RUNTIME_SMOKE_LLAMA_MODEL_PATH=/path/to/model.gguf \
    just runtime-smoke
  # Drop the GPU lines when you only need CPU coverage, or switch GPU policy to auto for best-effort accelerators.
  ```
- On Windows workstations with the CUDA build under `cache/llama.cpp/build-windows/bin/`, use `just runtime-smoke-gpu-real` to skip the stub stage, keep the run artifacts, and enforce the hardware-backed GPU path without retyping the environment variables.
- Prefer existing builds and lower resource impact:
  ```bash
  export RUNTIME_SMOKE_SKIP_BUILD=1          # never trigger cargo build
  export RUNTIME_SMOKE_USE_RELEASE=1         # prefer target/release/arw-server when present
  export RUNTIME_SMOKE_NICE=1                # run arw-server / llama-server under nice/ionice
  export ARW_WORKERS=1 ARW_WORKERS_MAX=1    # shrink worker pool to keep RAM flat
  just runtime-smoke
  ```
  Combine with `ARW_SERVER_BIN=/path/to/arw-server` when you already have a build artifact you want to reuse even across clean worktrees.
- Prefer a single command? `just runtime-smoke-safe` sources those defaults (GPU policy `skip`) and runs the suite in one go.

### Memory guardrails

The GPU smoke automatically estimates how much RAM the configured GGUF weights will consume and compares it against the free memory reported by `/proc/meminfo`. If headroom is too tight the helper stops before launching `llama-server` and announces the shortfall, then falls back to simulated GPU mode (unless you set `LLAMA_GPU_REQUIRE_REAL=1`, in which case the run exits with an error).

Memory thresholds can be tuned with environment variables:

- `RUNTIME_SMOKE_MEM_FACTOR` (default `2.2`) multiplies the GGUF size to account for working buffers.
- `RUNTIME_SMOKE_MEM_OVERHEAD_GB` (default `1`) adds a fixed cushion on top of the factor.
- `RUNTIME_SMOKE_MEM_RESERVE_GB` (default `1`) keeps system RAM free even if the factor check passes.
- `RUNTIME_SMOKE_MIN_REQUIRED_GB` (default `0`) enforces an absolute minimum requirement.
- `RUNTIME_SMOKE_ALLOW_HIGH_MEM=1` bypasses the guard entirely when you’re certain enough RAM is available.

Running on 16 GB hosts? Stick to 4-bit TinyLlama weights, keep `LLAMA_GPU_LAYERS` below 8, and leave the suite in `auto` mode so it falls back to simulated GPU coverage if buffers would exceed the available headroom.

Need a quick capacity check without launching anything? Use `RUNTIME_SMOKE_DRY_RUN=1 just runtime-smoke` (or the `runtime-smoke-dry-run` recipe) to print the preflight plan and memory guard hints.

When you want the temporary run directory to stick around for an investigation, export `RUNTIME_SMOKE_KEEP_TMP=1`. The helper writes a `.keep` marker so the automatic pruning that maintains `.smoke/runtime/` won’t delete the run later. Use `RUNTIME_SMOKE_KEEP_RECENT` and `RUNTIME_SMOKE_RETENTION_SECS` to tune how many historical runs the cleanup script keeps.

Need to understand how bundle updates roll out or how to roll back a bad release? See [Runtime Bundle Runbook](../ops/runtime_bundle_runbook.md) for the signed update cadence, manifest verification workflow, and the operator rollback checklist.

When you flip the guardrail on (`export ARW_REQUIRE_SIGNED_BUNDLES=1`), the server rejects bundle reloads unless every installed runtime carries a trusted signature. Pair it with `arw-cli runtime bundles audit --require-signed` before deployments to catch missing signatures locally, and keep the authorized public keys in `configs/runtime/bundle_signers.json` (or point `ARW_RUNTIME_BUNDLE_SIGNERS` to a bespoke registry) so the CLI and supervisor can distinguish `[trusted]` versus `[untrusted]` manifests.

For more detail on the runtime matrix and supervisor roadmap, see `docs/guide/runtime_matrix.md`.

## Mirrors
- TinyLlama Project (S3): https://tinyllama-downloads.s3.amazonaws.com/weights/TinyLlama-1.1B-Chat-q4_k_m.gguf (no authentication; verify SHA).
- HF Mirror (community): https://hf-mirror.com/TheBloke/TinyLlama-1.1B-Chat-GGUF/raw/main/TinyLlama-1.1B-Chat-q4_k_m.gguf (community proxy; verify checksums).

If your organization mirrors GGUF files internally, add any recommended URLs to `configs/runtime/model_sources.json` under the `mirrors` key. The runtime helper will list them for quick reference, and operators can switch sources without editing scripts. Include a `"checksum": "sha256:..."` value when possible so downloads are validated automatically.
