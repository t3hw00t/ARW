---
title: DeepSeek-OCR Pipeline
---

# DeepSeek-OCR Pipeline

Updated: 2025-10-24  
Status: Experimental (opt-in)  
Type: Runbook

The adaptive OCR pipeline can offload work to DeepSeek-OCR when the `ocr_compression` feature is enabled. This document covers how to stand up the compression service, wire in configuration, and monitor / backfill jobs.

## 1. Provision the DeepSeek-OCR endpoint

The server expects a simple HTTP endpoint that accepts JSON payloads of the form:

```json
{
  "path": "/absolute/path/to/preprocessed.png",
  "lang": "eng",
  "quality": "balanced",
  "preprocess_steps": ["grayscale", "downscale:max=1280 (2000x1200 -> 1280x768)"]
}
```

and returns

```json
{
  "text": "...markdown or text...",
  "lang": "eng",
  "blocks": [
    {"text": "...", "x": 10, "y": 24, "w": 120, "h": 32, "confidence": 0.97}
  ]
}
```

### Docker (single GPU)

```
docker run --rm \
  --gpus all \
  -p 8088:8088 \
  --env MODEL=deepseek-ai/DeepSeek-OCR \
  --env TOKENIZER=deepseek-ai/DeepSeek-OCR \
  ghcr.io/deepseek-ai/ocr-server:latest
```

The container serves `POST /v1/ocr`. Point `ARW_OCR_COMPRESSION_ENDPOINT=http://localhost:8088/v1/ocr`.

### Bare-metal / venv

```
python -m venv .venv
source .venv/bin/activate
pip install torch torchvision torchaudio --index-url https://download.pytorch.org/whl/cu118
pip install vllm==0.8.5 flash-attn==2.7.3
pip install git+https://github.com/deepseek-ai/DeepSeek-OCR.git
python -m deepseek_ocr.server --model deepseek-ai/DeepSeek-OCR --host 0.0.0.0 --port 8088
```

Tune `--tensor-parallel-size` and `--max-concurrency` to match GPU capacity. For CPU/offload experiments, start with `--device cpu --max-concurrency 1` (slow, but useful for functional tests).

## 2. Server configuration

Add these environment variables to the `arw-server` process:

```
# Required when ocr_compression feature is enabled
ARW_OCR_COMPRESSION_ENDPOINT=http://127.0.0.1:8088/v1/ocr

# Optional tuning knobs
ARW_OCR_COMPRESSION_TIMEOUT_SECS=180
ARW_OCR_DECODER_GPUS=1           # number of GPU cards dedicated to the decoder farm
ARW_OCR_DECODER_CAPACITY=0.9     # proportion of total decoder throughput available to ARW
ARW_GPU_VRAM_MB=24576            # hint when sysinfo cannot inspect VRAM (e.g. inside containers)
```

`ARW_OCR_DECODER_*` hints feed into the capability profile and are recorded in sidecars for auditability.

## 3. Observability

Review `/metrics` and ensure the following counters are scraped:

| Metric | Labels | Description |
| --- | --- | --- |
| `arw_ocr_runs_total` | `backend`, `quality`, `runtime` | Successful OCR executions |
| `arw_ocr_cache_hits_total` | `backend`, `quality` | Cache hits reused existing sidecars |
| `arw_ocr_preprocess_total` | `quality` | Lite-tier preprocessing operations |
| `arw_ocr_backend_fallbacks_total` | `from`, `to` | Backend downgrades (e.g., compression → legacy) |

Recommended alerts:
- Fallback rate (`from=vision_compression`) > 5% for ≥15 minutes.
- `arw_ocr_runs_total{backend="vision_compression"}` flatline for >60 minutes during active ingestion.

## 4. Batch reprocessing

Once the endpoint is stable, reprocess previously captured screenshots at higher fidelity:

```
arw-cli screenshots backfill-ocr \
  --backend vision_compression \
  --quality full \
  --refresh-capabilities \
  --base http://127.0.0.1:8091
```

Add `--prefer-low-power` on shared workstations so the planner respects thermal constraints. Use `--limit` and `--dry-run` to stage in small batches.

For scheduled runs, add a cron/CI step that:

1. Ensures the DeepSeek-OCR endpoint is reachable (`curl -f $ARW_OCR_COMPRESSION_ENDPOINT/health`).
2. Runs the command above.
3. Publishes metrics/summary via chat or dashboard.

## 5. Rollback strategy

- Remove `ARW_OCR_COMPRESSION_ENDPOINT` to force legacy-only behaviour.
- Monitor `arw_ocr_backend_fallbacks_total{from="vision_compression",to="legacy"}` — it spikes automatically when the endpoint disappears.
- Sidecars include `backend` and `backend_reason`; downstream agents can detect the last backend used and schedule future reprocess attempts.

Keep the legacy Tesseract path enabled on all builds until the compression backend is battle-tested.
