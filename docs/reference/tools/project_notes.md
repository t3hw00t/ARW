---
title: Project Notes Tools
---

# Project Notes Tools
Updated: 2025-10-16
Status: Enabled (`project.notes.append`)
Type: Reference

## `project.notes.append`
Append a Markdown snippet to a project's `NOTES.md` without manually fetching or rewriting the file.

Input
```
{
  "project": "demo",
  "screenshot_path": "/home/user/.arw/screenshots/2025/10/03/120455_screen.png", // optional
  "heading": "Daily Review",            // optional; defaults to timestamp when true
  "caption": "Error dialog",            // optional alt text when screenshot_path provided
  "note": "> Investigate before merging", // optional blockquote-style note
  "markdown": "![alt](images/demo.png)", // optional raw Markdown appended verbatim
  "timestamp": true                      // include heading timestamp when no custom heading
}
```
- `project` — required project identifier (same validation as `/projects`).
- `screenshot_path` — optional absolute path under `<state_dir>/screenshots/…`; when supplied the tool links to the screenshot using a relative path from the project root and emits alt text derived from `caption`.
- `heading` / `timestamp` — control the optional heading. If `heading` is omitted and `timestamp` is `true` (default) the tool emits `## Screenshot YYYY-MM-DD HH:MM:SS UTC`.
- `note` — optional free-form text rendered as a blockquote beneath the heading.
- `markdown` — optional raw Markdown appended after any generated blocks. This is useful when the caller already placed the capture inside the project (for example, `![screenshot](images/demo.png)`).

Output
```
{
  "ok": true,
  "proj": "demo",
  "sha256": "…",
  "bytes": 742,
  "modified": "2025-10-03T17:32:14.201Z",
  "corr_id": "b7f8…",
  "snippet": "## Daily Review\n![Error dialog](../../screenshots/2025/10/03/120455_screen.png)\n> Investigate before merging\n",
  "notes_path": "/home/user/.arw/projects/demo/NOTES.md",
  "timestamp_iso": "2025-10-03T17:32:14Z",
  "screenshot": {
    "absolute": "/home/user/.arw/screenshots/2025/10/03/120455_screen.png",
    "relative_to_project": "../../screenshots/2025/10/03/120455_screen.png",
    "relative_to_state": "screenshots/2025/10/03/120455_screen.png",
    "markdown_path": "../../screenshots/2025/10/03/120455_screen.png"
  },
  "heading": "Daily Review"
}
```

Behavior
- Emits the existing `projects.notes.saved` event and audit log entry, ensuring launch surfaces stay in sync.
- Honors optimistic concurrency via SHA-256 checks; if `NOTES.md` changes during the append, the tool reloads and retries (three attempts) before returning an error.
- Sanitizes Markdown alt text and normalizes path separators to forward slashes for portability.
- Ignores missing `screenshot_path` or `markdown` entries gracefully; if no content would be written, the tool records a timestamp line to avoid empty sections.

Security & policy
- Same access controls as the `/projects` APIs: requires admin privileges and respects project directory boundaries. No network egress.

Use cases
- Launcher "Save to project" now calls `project.notes.append` when the "Append to notes" preference is enabled, automatically linking captured screenshots in context.
- Agents can inject context-specific notes (headings, blockquotes, or checklist Markdown) without duplicating file I/O logic or worrying about concurrent edits.
