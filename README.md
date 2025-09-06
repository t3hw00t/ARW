# Agents Running Wild (ARW)

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
- See `docs/PROJECT_INSTRUCTIONS.md`, `docs/DEPLOYMENT.md`, `docs/STRUCTURE.md`, and `docs/HARDWARE_AND_MODELS.md`.

Notes
- Service listens on `http://127.0.0.1:8090` by default. Open `/debug` for a simple UI.
- Portable state defaults to `%LOCALAPPDATA%/arw` (configurable in `configs/default.toml`).
