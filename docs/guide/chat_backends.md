---
title: Chat Backends
---

# Chat Backends
{ .topic-trio style="--exp:.6; --complex:.6; --complicated:.5" data-exp=".6" data-complex=".6" data-complicated=".5" }

The ARW debug Chat UI can use simple synthetic replies (echo/reverse/time) or call real model backends when configured.

> **Managed runtimes**: The upcoming runtime manager can download and launch llama.cpp/ONNX/vLLM bundles automatically (CPU, CUDA, ROCm, Metal, DirectML, CoreML, Vulkan) and expose health via `/state/runtimes`. Preview bundle catalogs now live in `configs/runtime/bundles.*.json`; inspect them locally with `arw-cli runtime bundles list` or remotely with `arw-cli runtime bundles list --remote` / `GET /state/runtime/bundles` (add `--json` for scripting). The remote snapshot now reports both catalogs and any bundles staged under `<state_dir>/runtime/bundles`, and the Launcher runtime panel exposes start/stop/restart controls for those managed bundles. Until the supervisor ships with installers, the env variables below continue to work for manual setup.

Updated: 2025-10-12
Type: How‑to

## Synthetic (Default)

Without configuration the Chat panel replies with:
- echo — repeats your message
- reverse — reverses your message
- time — prefixes your message with the current timestamp

This mode is useful for verifying the UI flow and events.

## Llama.cpp Server

Run a local llama.cpp server and point ARW to it:

```bash
./server -m /path/to/model.gguf -c 4096 --host 127.0.0.1 --port 8080
export ARW_LLAMA_URL=http://127.0.0.1:8080
```

The service will POST to `ARW_LLAMA_URL/completion` with prompt caching enabled for KV/prefix reuse:

```json
{ "prompt": "...", "n_predict": 256, "cache_prompt": true, "temperature": 0.7 }
```

Tip: run llama.cpp with a persistent prompt cache file (e.g., `--prompt-cache llama.prompt.bin`) to reuse KV across sessions.
When you drive llama via `just runtime-smoke-cpu` (or `MODE=cpu scripts/runtime_llama_smoke.sh`)
the helper appends `--prompt-cache` automatically. Set
`LLAMA_PROMPT_CACHE_PATH=/path/to/cache.bin` before launching to reuse the same cache across
runs (or keep the default temp file if you only need an ephemeral smoke test). The smoke
helper also enforces a wall-clock limit (`RUNTIME_SMOKE_TIMEOUT_SECS`, defaulting to the shared
`SMOKE_TIMEOUT_SECS` or 600) so stalled runs terminate instead of hang. Set the timeout knobs
to `0` if you want an unbounded session during manual investigation.

Need a `llama-server` binary? The smoke suite now auto-builds one into
`cache/llama.cpp/build` the first time it is missing (set `RUNTIME_SMOKE_AUTO_BUILD_LLAMA=0`
to opt out or run `just runtime-llama-build` manually when you prefer an explicit compile step).

GPU mode - `just runtime-smoke-gpu` (hard-requires real accelerators via
`LLAMA_GPU_REQUIRE_REAL=1`) or `MODE=gpu scripts/runtime_llama_smoke.sh` - appends a small
`--gpu-layers` hint (override via `LLAMA_GPU_LAYERS`) when you don't provide your own
`LLAMA_SERVER_ARGS`. Set `LLAMA_GPU_LOG_PATTERN` to the regex that proves GPU execution
(defaults to a broad CUDA/Metal/Vulkan/DirectML/HIP catch-all) and flip `LLAMA_GPU_ENFORCE=1`
to make the smoke test fail if the pattern is missing. When you want to exercise the GPU lane
without real accelerators, run `just runtime-smoke-gpu-sim` (or set/allow
`LLAMA_GPU_SIMULATE=1`); the helper keeps the stub backend but injects the marker expected by
the log verifier so CI can cover the GPU path. Export `LLAMA_GPU_REQUIRE_REAL=1` if you invoke
the script manually and would rather fail than simulate. For CPU runs
that must stay offloaded
from accelerators, set `LLAMA_FORCE_CPU_LAYERS=1` so the helper adds `--gpu-layers 0` when the
launch arguments omit it.

When running inside a sandbox that blocks loopback sockets entirely, export
`ARW_SMOKE_USE_SYNTHETIC=1` to skip the runtime exercise (the script exits successfully after
logging that the smoke was bypassed). Without the override, the helper automatically detects
the restriction, prints a warning, and reports a skipped run.

Optional tuning knobs (forwarded verbatim when set):

- `ARW_LLAMA_N_PREDICT` (`1-8192`)
- `ARW_LLAMA_TOP_P` (`0.0-1.0`)
- `ARW_LLAMA_TOP_K` (`1-5000`)
- `ARW_LLAMA_MIN_P` (`0.0-1.0`)
- `ARW_LLAMA_REPEAT_PENALTY` (`0.0-4.0`)
- `ARW_LLAMA_STOP` (comma/newline separated stop sequences)

If the server returns `{ "content": "..." }`, ARW uses it. It also supports an OpenAI-like `{ choices[0].message.content }` fallback shape.

## OpenAI-Compatible API

If llama.cpp is not available, ARW can use an OpenAI-compatible Chat Completions API.

Environment variables:

- `ARW_OPENAI_API_KEY` (required)
- `ARW_OPENAI_BASE_URL` (optional, default `https://api.openai.com`)
- `ARW_OPENAI_MODEL` (optional, default `gpt-4o-mini`)
- `ARW_CHAT_SYSTEM_PROMPT` (optional, default `You are a helpful assistant.`)
- `ARW_OPENAI_MAX_TOKENS`, `ARW_OPENAI_TOP_P`, `ARW_OPENAI_FREQUENCY_PENALTY`, `ARW_OPENAI_PRESENCE_PENALTY`, `ARW_OPENAI_STOP` (optional overrides)
- `ARW_CHAT_DEFAULT_TEMPERATURE`, `ARW_CHAT_DEFAULT_VOTE_K` (override defaults when the request omits `temperature`/`vote_k`)

Example:

```bash
export ARW_OPENAI_API_KEY=sk-...
export ARW_OPENAI_MODEL=gpt-4o-mini
```

Requests are sent to `POST {ARW_OPENAI_BASE_URL}/v1/chat/completions` with a body like:

```json
{
  "model": "gpt-4o-mini",
  "messages": [ { "role": "user", "content": "Hello" } ],
  "temperature": 0.7
}
```

Optional fields that ARW forwards:

- `temperature` — overrides the sampling temperature (default `0.2`).
- `vote_k` — enables server-side self-consistency by sampling multiple candidates; ARW clamps this to `1..=5`.
- Environment overrides listed above — forwarded verbatim when present.
- When a request omits `temperature`/`vote_k`, `ARW_CHAT_DEFAULT_TEMPERATURE` and `ARW_CHAT_DEFAULT_VOTE_K` (if set) provide the fallback values.

### LiteLLM (OpenAI-compatible proxy)

ARW also supports LiteLLM by pointing the OpenAI-compatible base URL to your LiteLLM server.

Environment (either form works):

- `ARW_LITELLM_BASE_URL` (e.g., `http://127.0.0.1:4000`)
- `ARW_LITELLM_API_KEY` (optional, if your proxy requires it)
- `ARW_LITELLM_MODEL` (optional; otherwise `ARW_OPENAI_MODEL` is used)

If `ARW_LITELLM_*` vars are set, they take precedence over the `ARW_OPENAI_*` ones; when they are absent ARW automatically falls back to the OpenAI configuration.

## Request Options

`/admin/chat/send` (and the upcoming UI knob) accepts an optional `temperature` and `vote_k`. Both are forwarded to the configured backend when supported; otherwise ARW gracefully falls back to a single synthetic reply.

## UI

Open `/admin/debug`, select a model (echo/reverse/time), set Temperature if desired, and Send. When a backend is configured, the response content comes from the backend; otherwise the synthetic reply is used.
## Modes, Self‑Consistency, and Verifier (gated)

- Mode controls planner hints and optional execution:
  - Quick: no self‑consistency or verifier
  - Balanced: self‑consistency vote‑k=3
  - Deep: self‑consistency vote‑k=5
  - Verified: self‑consistency vote‑k=3 + verifier pass

- Gates (policy‑controlled):
  - `chat:self_consistency` — allow running vote‑k sampling
  - `chat:verify` — allow running a verifier pass

If a gate is denied, behavior degrades gracefully to a single pass. Planner metadata is returned regardless, under `assistant.planner` and also emitted on `chat.planner` events.
