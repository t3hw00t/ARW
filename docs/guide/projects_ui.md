---
title: Projects UI
---

# Projects UI

!!! warning "Legacy bridge only"
    The Projects UI currently ships only with the legacy `arw-svc` bridge.
    Start it with `scripts/start.sh --legacy` (Linux/macOS) or
    `scripts/start.ps1 -Legacy` (Windows) so the `/admin/ui/projects`
    endpoints are exposed. The unified `arw-server` remains headless-first on
    port 8091 and does not serve this UI yet.

The Projects UI gives you a simple, secure place to:

Updated: 2025-09-15
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

See also: [Keyboard Shortcuts](shortcuts.md)

## Enable and Open

- Set `ARW_DEBUG=1` (or provide `X-ARW-Admin` when running in locked mode).
- Start the legacy bridge so the admin UI is served:
  - Linux / macOS:
    ```bash
    ./scripts/start.sh --legacy
    ```
  - Windows:
    ```powershell
    powershell -ExecutionPolicy Bypass -File scripts\start.ps1 -Legacy
    ```
- Open the legacy admin UI at `http://127.0.0.1:8090/admin/ui/projects`.
  - If you only need the unified `arw-server` without the Projects UI, it
    continues to default to `http://127.0.0.1:8091/`.

!!! note "Legacy debug UI"
    Projects UI is part of the legacy `arw-svc` bridge on port 8090. The unified headless server on port 8091 exposes project APIs under `/projects/*` without this UI shell.

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

**Legacy bridge:** Agents can subscribe to `/admin/events` and react to project lifecycle to train, plan, scaffold, or run checks (admin‑gated SSE).
