---
title: UI Screenshot Tools
---

# UI Screenshot Tools
Updated: 2025-09-20
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
  "force": false     // optional; recompute even if a cached sidecar exists
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
  "cached": false
}
```

Confidence is reported per word when the OCR engine provides it. `-1` confidence values are omitted. If the requested language is unavailable, the tool falls back to `eng` and reports the effective language under `lang`. Results are cached per language in `<name>.ocr.<lang>.json` (see `ocr_path`); rerunning the tool reuses the sidecar unless `force` is set. The `cached` flag signals whether the response was served from disk, and `generated_at` marks the sidecar timestamp. Each run emits `screenshots.ocr.completed` with the payload above so launch surfaces can update alt text and search indexes without re-running OCR.

Security: capture and annotate require `io:screenshot`; OCR also requires `io:ocr`. No network egress.

See also: [Project Notes Tools](project_notes.md) for the `project.notes.append` macro that links captures into project notes.
