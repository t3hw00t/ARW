# Agent_Hub (Agents Running Wild - ARW)

Minimal Rust workspace for a local, user‑mode agent service and CLI.

Key goals
- Unintrusive, per‑user operation (portable mode supported)
- Simple HTTP service with event stream + debug UI
- Tool registration via macros + inventory
- Clear packaging for sharing/upload

Quickstart
- Build: `scripts/build.ps1` (Windows) or `scripts/build.sh` (Linux/macOS)
- Test:  `scripts/test.ps1` or `scripts/test.sh`
- Package: `scripts/package.ps1` or `scripts/package.sh` (creates `dist/` zip)

Docs
- Browse the user guide and developer docs with MkDocs.
  - Local: `pip install mkdocs mkdocs-material` then `mkdocs serve`
  - CI publishes to GitHub Pages (gh-pages branch) when pushing to `main`.
  - Source files live in `docs/` and are organized into Guide and Developer sections.

Notes
- Service listens on `http://127.0.0.1:8090` by default. Open `/debug` for a simple UI.
- Portable state defaults to `%LOCALAPPDATA%/arw` (configurable in `configs/default.toml`).
- To wire the UI to your hosted docs, set `ARW_DOCS_URL` (e.g., your GitHub Pages URL). The debug page will show mild “?” helps and a Docs button.
- With `ARW_DEBUG=1`, if a local docs site exists (`docs-site/` or `site/`), it is served at `/docs`.
