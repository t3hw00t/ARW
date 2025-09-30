---
title: Screenshots Gallery
---

# Screenshots Gallery

Updated: 2025-09-21
Type: How‑to

Open from the command palette (Ctrl/Cmd‑K → “Open Screenshots Gallery”). The gallery collects recent `screenshots.captured` events and displays thumbnails.

Actions per item
- Open: launch the file via the OS
- Copy path: copy absolute path
- Copy MD: copy a Markdown image link `![alt](path)`
- Save to project: prompt for project and destination path (defaults to `images/<filename>`), then import via `/projects/{proj}/import`
- Annotate: open overlay to draw rectangles; on Apply saves `*.ann.*` and sidecar `.ann.json`, updates the preview

Tips
- Use the “Capture screen/window/region” palette actions or Chat buttons to add new screenshots.
- OCR (optional): enable “Auto OCR” (Chat toggle) to extract text for instruction generation and to improve search. Results are stored per language in `<name>.ocr.<lang>.json` next to the image so the gallery and future search lanes can reuse them without re-running OCR; cached responses surface immediately unless `force: true` is supplied.
