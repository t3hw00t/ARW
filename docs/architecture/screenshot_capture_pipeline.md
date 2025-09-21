---
title: Screenshot Capture Pipeline
---

# Screenshot Capture Pipeline

Updated: 2025-09-21
Status: Planned
Type: Explanation

## Goals
- Give agents a safe, auditable way to present their on-screen context back to the user on demand.
- Keep the end-to-end flow responsive so the UI mirrors captures immediately.
- Preserve user privacy through explicit consent and lease-gated execution.

## Current Implementation Snapshot
1. **Service tools**
   - `ui.screenshot.capture` (always built) writes images under `.arw/screenshots/YYYY/MM/DD/` and publishes `screenshots.captured` SSE events with preview metadata.
   - `ui.screenshot.annotate_burn` blurs/highlights rectangles and keeps a JSON sidecar for edits.
   - `ui.screenshot.ocr` (feature `ocr_tesseract`) extracts text locally when the build enables it.
2. **Policy gates**
   - Capture/annotate require the `io:screenshot` lease; OCR requires `io:ocr`.
   - Runs log gate acquisitions in the Policy lane for auditability.
3. **Launcher integration**
   - Palette shortcuts and chat buttons trigger captures via `run_tool_admin` and render previews with annotate/OCR quick actions.
   - The Activity lane and Screenshots Gallery subscribe to `screenshots.captured` for instant thumbnails.
4. **Storage + reuse**
   - Files stay in per-day folders; gallery actions support copying Markdown links or importing images into a project workspace.

## Operational Plan
1. **Consent and UX safeguards**
   - Continue requiring explicit prompts (“Show me what you see”).
   - Keep capture scope choices (screen, window, region) in both chat buttons and palette entries.
   - Surface capture success/failure notifications so the user knows when a screenshot was taken.
2. **Reliability**
   - Monitor capture errors (missing display, permission denials) via telemetry; bubble user-facing guidance in the Activity lane when a capture fails.
   - Exercise the tool on macOS, Windows, and Linux builds during release smoke tests.
3. **Security & privacy**
   - Rotate `io:screenshot`/`io:ocr` leases with tight TTLs and record requester identity in audit logs.
   - Encourage agents to annotate/blur sensitive regions before re-sharing captures.
4. **Documentation & discoverability**
   - Keep the Screenshots Guide and tool reference pages aligned with implementation details.
   - Link relevant docs (guide, gallery, tool reference) from onboarding checklists for new agents.
5. **Future enhancements**
   - Optional live preview (WebRTC) exploration remains out-of-scope until capture reliability is mature.
   - Evaluate auto-redaction heuristics when OCR confidence surpasses a threshold.

## Harmonization Checklist
- [x] Specs and docs drop "planned" qualifiers for screenshot tools.
- [x] Guide, reference, and architecture plan describe the same tool parameters and gating.
- [x] Annotate flow documented alongside capture/ocr so agents can discover the full pipeline.
