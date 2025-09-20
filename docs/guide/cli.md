---
title: CLI Guide
---

# CLI Guide
Updated: 2025-09-20
Type: How‑to

Goal-oriented tasks using the `arw-cli` binary. This guide shows common commands with copy‑pasteable examples and flags you’re likely to want.

Examples assume the unified `arw-server` is running on `http://127.0.0.1:8091`.

Prereqs
- Build or install the workspace: `cargo build -p arw-cli --release`
- Ensure the service is built if you plan to interact with it, but most CLI commands are local utilities and do not require the service.

Basics
- Version and bootstrap
  - `arw-cli` — prints version, calls hello, and shows effective paths
- Ping
  - `arw-cli ping --base http://127.0.0.1:8091` — checks `/healthz` and `/about` and prints a JSON summary; `--admin-token` flag or `ARW_ADMIN_TOKEN` env adds Bearer
- Paths
  - `arw-cli paths` — JSON of effective `stateDir/cacheDir/logsDir` etc.
  - `arw-cli paths --pretty` — pretty‑printed JSON
- Tools
  - `arw-cli tools` — list registered tools (id/version/caps)
  - `arw-cli tools --pretty` — pretty JSON
 
Specs
- `arw-cli spec health --base http://127.0.0.1:8091 [--pretty]` — fetch `/spec/health` and print JSON (pretty‑print with `--pretty`)
- OpenAPI: `curl http://127.0.0.1:8091/spec/openapi.yaml` — served by `apps/arw-server/src/api_spec.rs`, returns the HTTP API contract (YAML)
- AsyncAPI: `curl http://127.0.0.1:8091/spec/asyncapi.yaml` — event stream schema aligned with the SSE bus
- Index: `curl http://127.0.0.1:8091/spec/index.json | jq` — lists available spec artifacts and JSON schemas

Events (SSE)
- Tail live events from the service with optional replay and prefix filters:
  - `arw-cli events tail --base http://127.0.0.1:8091 --replay 10 --prefix models. --prefix feedback.`
  - Add `--json-only` to print only the JSON payloads.

Gating Keys
- List all known keys used by `#[arw_gate]` and policy:
  - `arw-cli gate keys`

Policy Capsules
- Template
  - Pretty (default): `arw-cli capsule template` or `--pretty`
  - Compact: `arw-cli capsule template --compact`
- Generate ed25519 keypair (b64)
  - Print JSON summary: `arw-cli capsule gen-ed25519`
  - Include issuer: `arw-cli capsule gen-ed25519 --issuer "local-admin"`
  - Save keys to files:
    - `arw-cli capsule gen-ed25519 --out-pub pub.txt --out-priv priv.txt`
- Sign a capsule file
  - `arw-cli capsule sign-ed25519 <sk_b64> <capsule.json>` — prints signature (b64)
  - Save signature to file: `... --out signature.txt`
- Verify a signature
  - `arw-cli capsule verify-ed25519 <pk_b64> <capsule.json> <sig_b64>` — prints `ok` if valid (capsule’s `signature` field is ignored for verification)

Shell Completions
- Generate completions for your shell and either print to stdout or write to a directory:
  - Bash (print): `arw-cli completions bash`
  - Zsh (to dir): `arw-cli completions zsh --out-dir ~/.local/share/zsh/site-functions`
  - Fish: `arw-cli completions fish --out-dir ~/.config/fish/completions`
  - PowerShell: `arw-cli completions powershell --out-dir "$PROFILE_DIR"`
  - Elvish: `arw-cli completions elvish`

Install docs & completions (script)
- `scripts/install-cli-docs.sh` installs man page and completions into user‑local directories:
  - Man: `~/.local/share/man/man1/arw-cli.1`
  - Bash: `~/.local/share/bash-completion/completions`
  - Zsh: `~/.local/share/zsh/site-functions`
  - Fish: `~/.config/fish/completions`

Tips
- Keep the private key safe; only commit public keys and signed capsules (with `signature`) as needed.
- The service can adopt gating via capsules; see Security Hardening and Policy guides for how to apply.
- Admin surfaces live on the unified server; use `--base http://127.0.0.1:8091` when interacting with `/admin/*` or `/spec/*` routes locally.

Related
- Reference (commands and flags): [CLI Reference](../reference/cli.md)
- Security: [security_hardening.md](security_hardening.md), [GATING_KEYS.md](../GATING_KEYS.md), see also [Policy & Permissions](policy_permissions.md)
