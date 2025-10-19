# AI Assistants Index
Updated: 2025-10-09
Type: Reference

Microsummary: Map of repo for AI/code models: purpose, key files, how to run, limits. Stable entrypoint for tooling.

- Purpose: unified local-first agents service (`arw-server` in `apps/arw-server/`) plus tooling surfaces.
- Quick run: follow `docs/guide/quickstart.md` for the unified flow; use `scripts/debug.{sh,ps1} --open` with `ARW_DEBUG=1` to open `/admin/debug`.
- Key files:
  - Primary service: `apps/arw-server/` (Rust unified server).
  - CLI: `apps/arw-cli/`; desktop launcher: `apps/arw-launcher/` (drives the unified server by default).
  - Docs: `docs/` (MkDocs Material); site config: `mkdocs.yml`.
  - Schemas: `spec/schemas/`; OpenAPI preview: `docs/static/openapi.json`.
- Limits: default‑deny for write/shell/network; assume no internet; prefer explicit permissions.
- Shells: harness default is PowerShell—invoke commands with `["pwsh","-NoLogo","-NoProfile","-Command", ...]`, and only pivot to Git Bash/WSL after confirming they exist. Prefer the repo `.ps1`/`.cmd` wrappers for build/test work.
- Windows search fallback: when ripgrep is absent, pair `Get-ChildItem -Recurse` with `Select-String` and note the substitution in your status.
- WSL: keep the checkout under `/home/<user>`, use the `.sh` helpers, and run `bash scripts/env/switch.sh windows-wsl` (plus `scripts\env\switch.ps1 windows-host` when returning) so build artefacts don’t cross between environments.
