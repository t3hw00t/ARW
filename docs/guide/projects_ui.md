---
title: Projects UI
---

# Projects UI

!!! note "Debug flag"
    The Projects UI ships with the unified `arw-server`. Start the service with
    `ARW_DEBUG=1` (or provide `X-ARW-Admin`) so the `/admin/ui/projects`
    endpoints are exposed.

The Projects UI gives you a simple, secure place to:

Updated: 2025-09-20
Type: How‑to

- Create projects under a local folder.
- Take free‑form notes per project (`NOTES.md`).
- Browse each project’s folder tree (folders only; dotfiles hidden), with:
  - Breadcrumbs and Back to navigate quickly
  - Per‑project “last folder” restore on reopen
  - Filter box to narrow files/folders by name
  - Drag‑and‑drop upload into the current folder (≤10 MiB)
  - Open with OS handler or your preferred editor (global or per‑project)
  - Inline edit with conflict‑aware merge and visual diff
- Save captured screenshots from the launcher/gallery (the “Save to project” quick action targets this UI).

See also: [Keyboard Shortcuts](shortcuts.md)

## Enable and Open

- Set `ARW_DEBUG=1` (or provide `X-ARW-Admin` when running in locked mode).
- Start the unified server as usual (e.g. `bash scripts/start.sh --wait-health`).
- Open `http://127.0.0.1:8091/admin/ui/projects` in your browser.

!!! note "Admin gating"
    The Projects UI is served from `/admin/ui/projects` on port 8091. All `/projects/*`
    APIs remain admin-gated and mirror these interactions.

All `/projects/*` endpoints are treated as administrative and are protected by the service’s admin gate.

## Storage

- Base directory: `ARW_PROJECTS_DIR` (env). If unset, defaults to `<state_dir>/projects`.
- Notes file: `<project>/NOTES.md` (plaintext/Markdown).

## API

- `GET /projects/list` → `{ items: string[] }`
- `POST /projects/create` with `{ name }` → creates `<project>` and `NOTES.md`
- `GET /projects/notes?proj=<name>` → returns note text
- `POST /projects/notes?proj=<name>` with body as `text/plain` → saves notes
- `GET /projects/tree?proj=<name>&path=<relative>` → `{ items: { name, dir, rel }[] }`
- `GET /projects/file?proj=<name>&path=<relative>` → `{ path, sha256, content, abs_path }`
- `POST /projects/file?proj=<name>&path=<relative>` → write atomically
  - Body (JSON):
    - `content` (string, UTF‑8) or `content_b64` (string, Base64‑encoded bytes)
    - `prev_sha256` (optional) — if provided and mismatched, returns 409 Conflict

Notes

- Names are sanitized: letters, numbers, space, `-`, `_`, `.`; no leading dot.
- Tree listing hides dotfiles and directories outside the project root.
- Default per‑file payload limit is `ARW_PROJECT_MAX_FILE_MB` (MiB), defaults to 1 MiB.

## Editor Integration

- Global editor: Command palette → “Set preferred editor…”, e.g. `code --goto {path}`
- Per‑project editor: Files → Prefs → set editor command (overrides global)
- “Open in Editor” uses project → global → OS default.

## Notes Autosave

- Toggle per project; saves after a short pause and shows an inline “Saved” indicator.

## Drag‑and‑Drop Upload

- Drop files onto the Files panel to import into the current folder.
- For existing destinations, choose to overwrite or the UI will create a “(copy)” variant.
- Large files are skipped with a guard; adjust server max limit as needed via `ARW_PROJECT_MAX_FILE_MB`.

## Events for Agents

To harmonize with orchestration and autonomous workers, the service emits:

- `projects.created` with `{ name }`
- `projects.notes.saved` with `{ name }`

Agents can subscribe to `/admin/events` and react to project lifecycle to train, plan, scaffold, or run checks (admin-gated SSE).
