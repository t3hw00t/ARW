---
title: Screenshots
---

# Screenshots
Updated: 2025-09-14
Status: Enabled (capture), Optional OCR (build‑time)
Type: How‑to

Purpose: allow AI agents and the user to capture the screen or a window region, attach it to a conversation, and optionally run OCR for instruction generation.

Security & policy
- Gated under `io:screenshot` with TTL leases and scopes (display/window/region)
- Every capture is audited; scope and TTL are visible in the Policy lane
- OCR gated under `io:ocr` with its own TTL lease

Tool interface (proposed)
`ui.screenshot.capture(scope, format, downscale, annotate?) → { path, width, height, preview_b64 }`
- `scope`: `screen` | `display:n` | `window:<id>` | `region:x,y,w,h`
- `format`: `png` (default) or `jpg`
- `downscale`: optional max width for the preview (e.g., 640)
- `annotate`: optional rectangles/labels

Events
- On success, emit `screenshots.captured` with metadata for UI thumbnails

OCR (optional)
- `ui.screenshot.ocr(path) → { text, blocks[] }`
- Local OCR engine only; no network egress by default
- Build‑time feature: `ocr_tesseract` enables Tesseract via `leptess`. This repo enables it by default; install system deps (e.g., `tesseract-ocr` + `libtesseract-dev` on Linux) to build. Without the libs, disable with `--no-default-features` or build without `ocr_tesseract`.

Storage
- Default save directory: `.arw/screenshots/YYYY/MM/DD/<ts>_<scope>.png`

UI integration
- Sidecar Activity lane shows recent screenshots as thumbnails; click to open (launcher open path)
- Palette: “Capture screen (preview)” and “Capture this window (preview)”
- Chat: buttons for “Capture” and “Capture window”; inserts preview + path inline
- Auto OCR: toggle under the Chat composer; when on, OCR runs after capture and inserts extracted text under the preview.
- Gallery: open from the palette; shows recent captures with Open/Copy actions; “Save to project” copies the image into a selected project path (e.g., `images/…`).

Window capture
- Bounds: obtained via a Tauri command (`active_window_bounds`) that reports `x,y,w,h` for the active window.
- The capture tool uses `scope: region:x,y,w,h` for precise, relevant screenshots.

Region capture (drag)
- Press the palette action “Capture region (drag)” or the Chat button.
- An overlay appears; click‑drag to select a rectangle; Esc to cancel.
- The app translates the selection to absolute screen coordinates using the window bounds and devicePixelRatio before invoking `ui.screenshot.capture` with a `region:x,y,w,h` scope.

Annotations
- Click Annotate under a captured image (Chat or Gallery) to draw rectangles.
- On Apply, ARW writes a non‑destructive sidecar JSON (`.ann.json`) and saves a burned‑in annotated sibling image (`*.ann.png/jpg`).
- Blur is applied to annotated rectangles; a teal border is rendered for visibility.
- Copy Markdown: use the button to copy a Markdown image link. Alt text defaults to “screenshot”; consider pasting OCR’s first line as alt text.

Notes: design accepted; tooling stubs and UI thumbnails planned.
