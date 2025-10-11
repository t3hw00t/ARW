---
title: Runtime Quickstart (Non-Technical)
---

# Runtime Quickstart (Non-Technical)
Updated: 2025-10-12
Type: Tutorial

This walkthrough is aimed at operators who want to validate the managed runtime supervisor without digging into the codebase. It uses the automation we ship (`just` commands and helper scripts) so you can prepare weights and run the smoke test with minimal manual tinkering.

## Prerequisites

- Terminal access to the ARW workspace (this directory contains `Justfile`).
- A Hugging Face account and a “Read” access token. Create one at <https://huggingface.co/settings/tokens>.
- llama.cpp binaries compiled locally (see `docs/guide/runtime_matrix.md#building-llamacpp`). If you followed the standard instructions the binary lives at `cache/llama.cpp/build/bin/llama-server`.

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
4. Launch the GPU smoke test.

```bash
just runtime-check
just runtime-check-weights-only  # download weights without running the smoke
```

Follow the on-screen prompts. If you are missing a compiled `llama-server` binary, you can stop here and revisit the build instructions later—the helper will fall back to simulated GPU mode so you can still verify the pipeline end-to-end.

## 3. Review the results

At the end of the run you will see:

- log locations (under `/tmp/arw-runtime-smoke.*` by default),
- whether the GPU acceleration markers were detected,
- any policy gating or restart-budget warnings emitted by the supervisor.

If the helper used the simulated mode (because no real binary or weights were available) you can rerun `just runtime-check` (or `just runtime-check-weights-only`) after resolving the prerequisites; cached weights are reused automatically. You can adjust the upstream sources in `configs/runtime/model_sources.json` if your organization mirrors weights internally.

## Advanced options

- Download weights only:
  ```bash
  just runtime-weights
  ```
- Supply custom Hugging Face sources:
  ```bash
  LLAMA_MODEL_SOURCES="repo::file,repo2::file2" just runtime-weights
  ```
- Provide a checksum for any source by adding a `"checksum": "sha256:..."` entry in `configs/runtime/model_sources.json`; the helper validates downloads automatically when a checksum is present.
- Opt-in to automatic Hugging Face downloads from the smoke helper:
  ```bash
  LLAMA_ALLOW_DOWNLOADS=1 MODE=gpu LLAMA_SERVER_BIN=/path/to/llama-server just runtime-smoke
  ```
- List configured mirrors without downloading anything:
  ```bash
  just runtime-mirrors

  # include -- --check to send a HEAD request to each mirror
  just runtime-mirrors -- --check
  ```
- Skip the guided flow and run the smoke directly (requires all env vars to be set):
  ```bash
  MODE=gpu LLAMA_SERVER_BIN=/path/to/llama-server just runtime-smoke
  ```

For more detail on the runtime matrix and supervisor roadmap, see `docs/guide/runtime_matrix.md`.

## Mirrors
- TinyLlama Project (S3): https://tinyllama-downloads.s3.amazonaws.com/weights/TinyLlama-1.1B-Chat-q4_k_m.gguf (no authentication; verify SHA).
- HF Mirror (community): https://hf-mirror.com/TheBloke/TinyLlama-1.1B-Chat-GGUF/raw/main/TinyLlama-1.1B-Chat-q4_k_m.gguf (community proxy; verify checksums).

If your organization mirrors GGUF files internally, add any recommended URLs to `configs/runtime/model_sources.json` under the `mirrors` key. The runtime helper will list them for quick reference, and operators can switch sources without editing scripts. Include a `"checksum": "sha256:..."` value when possible so downloads are validated automatically.
