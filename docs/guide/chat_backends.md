---
title: Chat Backends
---

# Chat Backends
{ .topic-trio style="--exp:.6; --complex:.6; --complicated:.5" data-exp=".6" data-complex=".6" data-complicated=".5" }

The ARW debug Chat UI can use simple synthetic replies (echo/reverse/time) or call real model backends when configured.

> **Managed runtimes**: The upcoming runtime manager can download and launch llama.cpp/ONNX/vLLM bundles automatically (CPU, CUDA, ROCm, Metal, DirectML, CoreML, Vulkan) and expose health via `/state/runtimes`. Until that ships, the env variables below continue to work for manual setup.

Updated: 2025-09-22
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
When you drive llama via `scripts/runtime_llama_smoke.sh MODE=real` the helper now appends
`--prompt-cache` automatically. Set `LLAMA_PROMPT_CACHE_PATH=/path/to/cache.bin` before
launching to reuse the same cache across runs (or keep the default temp file if you only
need an ephemeral smoke test). The smoke helper also enforces a wall-clock limit
(`RUNTIME_SMOKE_TIMEOUT_SECS`, defaulting to the shared `SMOKE_TIMEOUT_SECS` or 600) so
stalled runs terminate instead of hang. Set the timeout knobs to `0` if you want an
unbounded session during manual investigation.

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
