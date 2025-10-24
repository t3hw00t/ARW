---
title: UI Screenshot Tools
---

# UI Screenshot Tools
Updated: 2025-10-16
Status: Enabled (`ui.screenshot.capture`, `ui.screenshot.annotate_burn`); Optional OCR (`ui.screenshot.ocr`)
Type: Reference

## `ui.screenshot.capture`
Capture the screen/display/window/region and return a file path and a small preview.

Input
```
{
  "scope": "screen" | "display:0" | "window:main" | "region:100,100,800,600",
  "format": "png" | "jpg",
  "downscale": 640
}
```

Output
```
{ "path": ".arw/screenshots/2025/09/14/..png", "width": 1920, "height": 1080, "preview_b64": "data:image/png;base64,..." }
```

Emits `screenshots.captured` with metadata (path, dims) and optional `preview_b64` for UI.

## `ui.screenshot.annotate_burn`
Apply blur/highlight rectangles to an existing screenshot, producing a sibling image and preview.

Input
```
{
  "path": ".arw/screenshots/...png",
  "annotate": [ { "x":10, "y":20, "w":100, "h":40, "blur": true } ],
  "downscale": 640
}
```

Output
```
{
  "path": ".arw/screenshots/2025/09/14/..ann.png",
  "ann_path": ".arw/screenshots/2025/09/14/..ann.json",
  "width": 1920,
  "height": 1080,
  "preview_b64": "data:image/png;base64,..."
}
```

## `ui.screenshot.ocr` (optional)
Extract text from an image using a local OCR engine.

Input
```
{
  "path": ".arw/screenshots/...png",
  "lang": "eng",   // optional; tessdata language code, defaults to "eng"
  "force": false,     // optional; recompute even if a cached sidecar exists
  "backend": "legacy" | "vision_compression", // optional; request a specific OCR backend
  "quality": "lite" | "balanced" | "full",    // optional; request compression/accuracy tier
  "prefer_low_power": false,                  // optional; hint that battery/thermal limits matter
  "refresh_capabilities": false               // optional; force a capability refresh before running
}
```

Output
```
{
  "text": "...",
  "blocks": [
    { "text":"...", "x":.., "y":.., "w":.., "h":.., "confidence": 92.1 }
  ],
  "lang": "eng",
  "source_path": ".arw/screenshots/...png",
  "ocr_path": ".arw/screenshots/...ocr.eng.json",
  "generated_at": "2025-09-29T15:45:00Z",
  "cached": false,
  "backend": "legacy",
  "backend_reason": "legacy backend requested/available",
  "backend_supported": true,
  "quality_tier": "balanced",
  "quality_reason": "legacy backend heuristics for total_mem_mb=8192 logical_cpus=8",
  "runtime_class": "cpu_balanced",
  "runtime_reason": "legacy backend in balanced mode",
  "compression_target": null,
  "expected_quality": 0.97,
  "confidence_hint": 0.95,
  "preprocess_steps": [],
  "capability_profile": {
    "total_mem_mb": 8192,
    "available_mem_mb": 5120,
    "logical_cpus": 8,
    "physical_cpus": 4,
    "gpu_vram_mb": 4096,
    "gpu_kind": "dedicated",
    "decoder_gpus": null,
    "decoder_capacity": null,
    "low_power_hint": false,
    "os": "windows",
    "collected_at": "2025-10-24T09:42:00Z"
  }
}
```

Confidence is reported per word when the OCR engine provides it. `-1` confidence values are omitted. If the requested language is unavailable, the tool falls back to `eng` and reports the effective language under `lang`. Results are cached per language in `<name>.ocr.<lang>.json` (see `ocr_path`); rerunning the tool reuses the sidecar unless `force` is set **and** the cached metadata (`backend`, `quality_tier`) matches the current run plan. The `cached` flag signals whether the response was served from disk, and `generated_at` marks the sidecar timestamp. Metadata fields (`backend`, `quality_tier`, `runtime_class`, reasons, `expected_quality`, `confidence_hint`, `preprocess_steps`, and the full `capability_profile`) describe how the adaptive pipeline interpreted the host device. Each run emits `screenshots.ocr.completed` with the payload above so launch surfaces can update alt text and search indexes without re-running OCR, and downstream agents can decide whether to recompute at a higher quality tier.

### Dynamic capability detection

`ui.screenshot.ocr` interrogates the host (memory, CPU count, optional GPU VRAM and decoder hints) and blends that with caller preferences (`backend`, `quality`, `prefer_low_power`). On low-end devices the service automatically drops to a lite tier and records that decision; on higher-end hardware it keeps the balanced default and, when the vision-compression backend is compiled in, can target aggressive token compression ratios. Use the metadata fields to audit which tier ran and to schedule reprocessing jobs on beefier machines when needed.

### Vision compression backend

- Build the server with `--features ocr_compression`.
- Set `ARW_OCR_COMPRESSION_ENDPOINT` to a local or remote DeepSeek-OCR inference service. Optional knobs: `ARW_OCR_COMPRESSION_TIMEOUT_SECS`, `ARW_OCR_DECODER_GPUS`, `ARW_OCR_DECODER_CAPACITY`, and `ARW_GPU_VRAM_MB`/`BYTES`.
- When the endpoint is unavailable at runtime the server falls back to the legacy backend, records the fallback reason, and emits the `arw_ocr_backend_fallbacks_total` counter.

### Telemetry

- `arw_ocr_runs_total{backend,quality,runtime}` — completed OCR executions.
- `arw_ocr_cache_hits_total{backend,quality}` — cache hits avoided recomputation.
- `arw_ocr_preprocess_total{quality}` — lite-tier preprocessing events.
- `arw_ocr_backend_fallbacks_total{from,to}` — runtime backend downgrades.

### Batch reprocessing

Use the CLI’s new hints to queue high-fidelity reprocessing when upgraded hardware is available:

```
arw-cli screenshots backfill-ocr \
  --backend vision_compression \
  --quality full \
  --refresh-capabilities \
  --base http://server:8091
```

Combine `--prefer_low_power` or `--force` as needed. The command respects cached metadata, so only sidecars produced at lower tiers will be regenerated.

Security: capture and annotate require `io:screenshot`; OCR also requires `io:ocr`. No network egress.

See also: [Project Notes Tools](project_notes.md) for the `project.notes.append` macro that links captures into project notes.
