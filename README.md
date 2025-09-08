# Agent_Hub (Agents Running Wild - ARW)

Minimal Rust workspace for a local, user‑mode agent service and CLI.

Key goals
- Unintrusive, per‑user operation (portable mode supported)
- Simple HTTP service with event stream + debug UI
- Tool registration via macros + inventory
- Clear packaging for sharing/upload

Quickstart
- Install [Nix](https://nixos.org/download.html) and use it as the entry point for building, testing, and packaging:
  - `nix develop` to enter a shell with all dependencies
  - Run commands directly: `nix develop --command cargo build`, `nix develop --command cargo test`, `nix develop --command scripts/package.sh`, etc.
- One‑shot setup (build, docs, package):
  - Windows: `powershell -ExecutionPolicy Bypass -File scripts/setup.ps1`
  - Linux/macOS: `bash scripts/setup.sh`
- Start the service with options:
  - Windows: `powershell -ExecutionPolicy Bypass -File scripts/start.ps1 -Debug -Port 8090 -DocsUrl https://your-pages -AdminToken secret`
  - Linux/macOS: `bash scripts/start.sh --debug --port 8090 --docs-url https://your-pages --admin-token secret`
- Minimal tray app (optional): run `arw-tray` from `target/release/` or from `dist/.../bin/` to start/stop the service, open the Debug UI, or quit from the system tray.
- Traditional scripts (fine‑grained):
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
- Sensitive endpoints (`/debug`, `/probe`, `/memory*`, `/models*`, `/governor*`, `/introspect*`, `/chat*`, `/feedback*`) are gated. Development: set `ARW_DEBUG=1`. Hardened: set `ARW_ADMIN_TOKEN` and send header `X-ARW-Admin: <token>`.
