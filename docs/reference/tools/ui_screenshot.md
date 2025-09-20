---
title: UI Screenshot Tools
---

# UI Screenshot Tools
Updated: 2025-09-17
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
{ "path": ".arw/screenshots/...png" }
```

Output
```
{ "text": "...", "blocks": [ { "text":"...", "x":.., "y":.., "w":.., "h":.. } ] }
```

Security: capture and annotate require `io:screenshot`; OCR also requires `io:ocr`. No network egress.
