---
title: Chat Backends
---

# Chat Backends
{ .topic-trio style="--exp:.6; --complex:.6; --complicated:.5" data-exp=".6" data-complex=".6" data-complicated=".5" }

The ARW debug Chat UI can use simple synthetic replies (echo/reverse/time) or call real model backends when configured.

Updated: 2025-09-12

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
{ "prompt": "...", "n_predict": 128, "cache_prompt": true, "temperature": 0.7 }
```

Tip: run llama.cpp with a persistent prompt cache file (e.g., `--prompt-cache llama.prompt.bin`) to reuse KV across sessions.

If the server returns `{ "content": "..." }`, ARW uses it. It also supports an OpenAI-like `{ choices[0].message.content }` fallback shape.

## OpenAI-Compatible API

If llama.cpp is not available, ARW can use an OpenAI-compatible Chat Completions API.

Environment variables:

- `ARW_OPENAI_API_KEY` (required)
- `ARW_OPENAI_BASE_URL` (optional, default `https://api.openai.com`)
- `ARW_OPENAI_MODEL` (optional, default `gpt-4o-mini`)

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

## Temperature

`/chat/send` accepts an optional `temperature` value. The service includes it in the assistant message and passes it through to backends when set.

## UI

Open `/debug`, select a model (echo/reverse/time), set Temperature if desired, and Send. When a backend is configured, the response content comes from the backend; otherwise the synthetic reply is used.
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
