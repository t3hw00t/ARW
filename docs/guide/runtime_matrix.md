---
title: Runtime Matrix
---

# Runtime Matrix

> Need the short version? See [Runtime Quickstart (Non-Technical)](runtime_quickstart.md).
Updated: 2025-10-12
Type: Blueprint
Status: In progress

ARW seeds a runtime matrix read-model from `runtime.health` events. Today it merges per-node HTTP telemetry with runtime registry states and accelerator summaries while the full grid of models and hardware remains under active development.

## Current state
- Local node health published every five seconds; payloads now merge HTTP telemetry with the runtime registry snapshot so readiness, degraded/error counts, and restart pressure all travel together.
- Read-model payloads include `ttl_seconds` so dashboards know exactly how long a snapshot should be treated as fresh before polling or prompting for an updated heartbeat.
- Accelerator rollups highlight CPU/GPU/NPU availability and the state mix per accelerator so operators can spot when a GPU lane degrades or drops offline.
- Node identifiers resolve from `ARW_NODE_ID` (or fallback hostname) and feed the runtime matrix read-model.
- Accessibility strings accompany each status so dashboards can surface the same context.
- Restart budgets surface remaining automatic restarts, the configured window, and the reset horizon so operators can decide when to intervene or widen the budget.
- Launcher now mirrors the snapshot with a header badge in Project Hub, highlighting readiness counts, restart headroom, and next reset.
- CLI shortcuts: `arw-cli runtime status` prints the same snapshot (or `--json` for raw), and `arw-cli runtime restore --id <runtime>` triggers supervised restores while echoing the remaining budget or budget exhaustion.
  - Text mode now reports the active `ttl_seconds` so operators know when the matrix snapshot should be considered stale.
  - JSON mode emits `{ "supervisor": ..., "matrix": ... }` so scripts can consume both views (including `ttl_seconds`) in one call.
- Smoke check: `just runtime-smoke` launches a stub llama endpoint, points the server at it, and verifies `chat.respond` flows end-to-end without needing model weights (extend with MODE=real once hardware-backed smoke rigs land). The helper exits automatically after `RUNTIME_SMOKE_TIMEOUT_SECS` seconds (defaults to the shared `SMOKE_TIMEOUT_SECS`, falling back to 600). Set either knob to `0` to disable the guard during manual debugging.
  - The smoke now also fetches `/state/runtime_matrix` and asserts every snapshot carries the accessible status strings (`label`, `detail`, `aria_hint`, `severity_label`) plus a fresh `runtime.updated` timestamp and positive `ttl_seconds`, catching regressions in the matrix feed before they escape CI.
  - To exercise a real llama.cpp build: `MODE=real LLAMA_SERVER_BIN=/path/to/server LLAMA_MODEL_PATH=/path/to/model.gguf just runtime-smoke`. Optionally pass `LLAMA_SERVER_ARGS="--your --flags"` or `LLAMA_SERVER_PORT=XXXX` to match your deployment.
  - CI parity (`scripts/dev.sh verify --ci`) runs both the stub path and a simulated GPU mode (`LLAMA_GPU_SIMULATE=1 MODE=gpu`) to keep the accelerator detection and log parsing code paths covered even when real GPUs are absent.
  - GPU runs allocate a dedicated workspace under `.smoke/runtime/run.*` (override with `RUNTIME_SMOKE_ROOT`). Automatic pruning keeps the newest six runs unless you change `RUNTIME_SMOKE_KEEP_RECENT`/`RUNTIME_SMOKE_RETENTION_SECS` or pin an investigation with `RUNTIME_SMOKE_KEEP_TMP=1`.
  - Before launching `llama-server`, the helper estimates RAM usage from the GGUF size. If `MemAvailable` minus `RUNTIME_SMOKE_MEM_RESERVE_GB` falls short of `size × RUNTIME_SMOKE_MEM_FACTOR + RUNTIME_SMOKE_MEM_OVERHEAD_GB`, the script downgrades to the simulated GPU path and prints the shortfall (set `RUNTIME_SMOKE_ALLOW_HIGH_MEM=1` to bypass, or tweak the factors to match your hardware). With `LLAMA_GPU_REQUIRE_REAL=1`, the guard aborts instead of silently switching paths so CI never reports a false positive.
- Vision smoke: `just runtime-smoke-vision` uses the managed supervisor to launch a stub llava runtime from a generated manifest, probes `/describe`, forces a restore, and watches `/state/runtime_matrix` for the vision runtime to cycle back to Ready. The helper sets `ARW_SMOKE_MODE=vision` so only the runtimes/read-models needed for the smoke are launched and writes under `.smoke/vision/run.XXXX/` by default; export `VISION_SMOKE_ROOT=/path/to/cache` if you want to redirect or reuse the working directory between runs. Provide `ARW_SERVER_BIN=/path/to/arw-server` if you want to reuse a custom build; otherwise the helper builds the binary automatically. The latest pass also asserts that the matrix payload carries the accessibility strings (`label`, `severity_label`, `aria_hint`, populated `detail`) and a positive `ttl_seconds` so dashboards do not regress silently.

### Getting real weights

When you are ready to run the runtime smoke against a real llama.cpp binary (CPU or GPU), download a GGUF checkpoint using a Hugging Face access token. The fastest path is `just runtime-weights`, which pulls the default TinyLlama weights into `cache/models/`. If you prefer the manual route:

1. Sign in to (or create) a Hugging Face account at https://huggingface.co/.
2. Generate a “Read” access token at https://huggingface.co/settings/tokens.
3. Export the token in your shell (`export HF_TOKEN=hf_...`) or run `huggingface-cli login`.
4. Fetch the desired weights, for example:
   ```bash
   huggingface-cli download ggml-org/tinyllama-1.1b-chat \
     --include tinyllama-1.1b-chat-q4_k_m.gguf \
     --local-dir ./models
   ```
5. Point the smoke test at the compiled server and the downloaded GGUF:
   ```bash
   MODE=gpu \
   LLAMA_SERVER_BIN=/path/to/llama.cpp/build/bin/llama-server \
   LLAMA_MODEL_PATH=$PWD/models/tinyllama-1.1b-chat-q4_k_m.gguf \
   just runtime-smoke
   ```

If the smoke script detects that a real run is missing weights, it now prints the same checklist so operators know how to proceed.

The helper still enumerates a small roster of public Hugging Face sources (`ggml-org/tinyllama-1.1b-chat`, `TheBloke/TinyLlama-1.1B-Chat-GGUF`, …) so existing downloads in `cache/models/<file>` can be re-used automatically. Set `LLAMA_MODEL_SOURCES="repo::file,repo2::file2"` when you want to exercise different checkpoints. Automatic downloads now require an explicit opt-in (`LLAMA_ALLOW_DOWNLOADS=1`) so CI and constrained sandboxes do not unexpectedly pull gigabytes of weights; otherwise the script prints the checklist shown above and exits or falls back to the stub backend. Configure organization-wide defaults and optional `checksum` values in `configs/runtime/model_sources.json`—the helper prints those mirrors (for example, the zero-auth TinyLlama S3 bucket) and validates downloads automatically whenever a checksum is present.

Running inside locked-down sandboxes sometimes blocks loopback sockets entirely. When the helper detects that scenario it now reports a “skipped” outcome instead of pretending the smoke passed; export `RUNTIME_SMOKE_REQUIRE_LOOPBACK=1` when you would rather fail the run so upstream automation can flag the gap.

### Example payload
```json
{
  "items": {
    "local": {
      "status": {
        "code": "ok",
        "severity": "info",
        "severity_label": "Info",
        "label": "Ready - Runtime telemetry nominal",
        "detail": [
          "Running within expected ranges"
        ],
        "aria_hint": "Runtime status Ready - Runtime telemetry nominal. Running within expected ranges"
      },
      "runtime": {
        "total": 1,
        "updated": "2025-10-04T12:34:56.789Z"
      }
    }
  },
  "ttl_seconds": 60
}
```
The accessible `severity_label` travels alongside the slug (`severity`) so launchers and assistive technologies can narrate the status without maintaining their own enum mapping.

## Roadmap
- Grid of Models × Hardware × Sandboxes with live health/throughput and load per cell.
- Pin preferred combos per agent/project and fall back automatically under policy control.
- Event-driven updates across `runtime.health`, `models.changed`, and `policy.*` topics.
- Quick actions: (re)load model, switch runtime/sandbox, open logs.
- Capability-aware leases per cell (gpu, sandbox:<kind>) with TTL expirations.
- Backends: local llama.cpp, planned ONNX Runtime/vLLM/whisper adapters, and opt-in remote fallbacks.

Telemetry payloads will continue to grow: aggregated HTTP error counts, request totals, derived error rates, slow-route annotations, runtime-state rollups, and accelerator summaries help dashboards distinguish latency spikes from outright failures while the matrix expands to multi-runtime tracking.

See also: Performance & Reasoning Playbook for how the scheduler selects Quick/Balanced/Deep/Verified against SLOs and budgets.
