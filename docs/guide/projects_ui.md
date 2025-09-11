# Projects UI

The Projects UI gives you a simple, secure place to:

- Create projects under a local folder.
- Take free‑form notes per project (`NOTES.md`).
- Browse each project’s folder tree (folders only; dotfiles hidden).

## Enable and open

- Set `ARW_DEBUG=1` (or provide `X-ARW-Admin` when running in locked mode).
- Start `arw-svc` and open:
  - `http://127.0.0.1:8090/ui/projects`

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

Notes

- Names are sanitized: letters, numbers, space, `-`, `_`, `.`; no leading dot.
- Tree listing hides dotfiles and directories outside the project root.

## Events for agents

To harmonize with orchestration and autonomous workers, the service emits:

- `Projects.Created` with `{ name }`
- `Projects.NotesSaved` with `{ name }`

Agents can subscribe to `/events` and react to project lifecycle to train, plan, scaffold, or run checks.

