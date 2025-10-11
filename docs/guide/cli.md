---
title: CLI Guide
---

# CLI Guide
Updated: 2025-10-11
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
- Managed runtime bundles
  - Local catalogs: `arw-cli runtime bundles list --dir ./configs/runtime --pretty`
  - Alternate install root: `arw-cli runtime bundles list --install-dir ~/.cache/arw/bundles`
  - Remote snapshot (catalogs + installed bundles): `arw-cli runtime bundles list --remote --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN`
  - Trigger rescan: `arw-cli runtime bundles reload --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN`
  - Start a managed runtime: `arw-cli runtime restore llama.cpp-preview/linux-x86_64-cpu --no-restart`
  - Stop a managed runtime: `arw-cli runtime shutdown llama.cpp-preview/linux-x86_64-cpu`
  - Download artifacts: `arw-cli runtime bundles install llama.cpp-preview/linux-x86_64-cpu`
  - Offline import: `arw-cli runtime bundles import --bundle llama.cpp-preview/linux-x86_64-cpu ./llama-build`
  - Roll back to a previous snapshot: `arw-cli runtime bundles rollback --bundle llama.cpp-preview/linux-x86_64-cpu --list`
  - Add `--json`/`--pretty` to the rollback command for machine-readable history listings or outcome summaries
  - Preview installs: add `--dry-run` to either command; use `--dest /custom/path` when staging bundles outside the default `<state_dir>/runtime/bundles`
  - Scripting: add `--json`/`--pretty` to either command; remote mode fetches `/state/runtime/bundles` (including `installations`) from the running server.

Admin Tokens
- Generate, hash, and persist `ARW_ADMIN_TOKEN` values without committing secrets:
  - `arw-cli admin token generate --length 32 --hash-env` — print a random token (hex) plus its SHA-256 hash as `ARW_ADMIN_TOKEN_SHA256=…`
  - `arw-cli admin token hash --read-env ARW_ADMIN_TOKEN` — hash an existing token from the environment (use `--stdin` to pipe secrets securely)
  - `arw-cli admin token persist --path .env --hash` — write `ARW_ADMIN_TOKEN` (and the hash) into a local env file, creating parent directories with `0600` permissions; omit `--token` to generate a fresh secret, or reuse a value with `--token ...` / `--read-env VAR`
  - Add `--print-token`/`--print-hash` when you need to surface the new values after persisting; defaults keep secrets in the file only.

Screenshots
- Backfill OCR sidecars for all captures (per language):
  - `arw-cli screenshots backfill-ocr --lang eng`
  - Add `--dry-run` to see which files would run, `--force` to recompute even when cached, `--limit 10` for spot checks.
  - Uses `/admin/tools/run` so set `ARW_ADMIN_TOKEN` or pass `--admin-token`.

Specs
- `arw-cli spec health --base http://127.0.0.1:8091 [--pretty]` — fetch `/spec/health` and print JSON (pretty-print with `--pretty`)
- OpenAPI: `curl http://127.0.0.1:8091/spec/openapi.yaml` — served by `apps/arw-server/src/api/spec.rs`, returns the HTTP API contract (YAML)
- AsyncAPI: `curl http://127.0.0.1:8091/spec/asyncapi.yaml` — event stream schema aligned with the SSE bus
- Index: `curl http://127.0.0.1:8091/spec/index.json | jq` — lists available spec artifacts and JSON schemas

State Snapshots
- Cluster registry: `arw-cli state cluster --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN` prints a table of nodes, their last advertisement, and whether they are stale compared to the server’s TTL (360 s). Add `--json --pretty` for raw inspection.
- Identity registry: `arw-cli state identity --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN` summarizes principals sourced from config files and environment. Include `--json` for machine-friendly output.

Events (SSE)
- Tail live events with `curl -N -H "Authorization: Bearer $ARW_ADMIN_TOKEN" "http://127.0.0.1:8091/events?replay=10&prefix=models."`
- When `ARW_ADMIN_TOKEN` is set, include `-H "Authorization: Bearer $ARW_ADMIN_TOKEN"`.

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
- Inspect active capsules
  - `arw-cli capsule status --limit 10`
  - JSON mode: `arw-cli capsule status --json --pretty`
- Emergency teardown (admin)
  - Preview: `arw-cli capsule teardown --id capsule-http --dry-run`
  - Remove all: `arw-cli capsule teardown --all --reason "reset misconfigured policy"`
  - Combine with `--json` to capture audit log entries.

Autonomy Lanes
- List current lanes and summaries: `arw-cli admin autonomy lanes --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN`
- Inspect a specific lane (JSON ready): `arw-cli admin autonomy lane --lane trial-g4-autonomy --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN --json --pretty`
- Pause and resume with operator metadata:
  - Pause: `arw-cli admin autonomy pause --lane trial-g4-autonomy --operator sevi --reason "Investigating drift"`
  - Resume to guided mode: `arw-cli admin autonomy resume --lane trial-g4-autonomy --mode guided --operator sevi --reason "Rollback complete"`
- Stop and flush all queued/in-flight jobs: `arw-cli admin autonomy stop --lane trial-g4-autonomy --operator sevi --reason "Budget exhausted"`
- Flush only queued jobs (leave running work for investigation): `arw-cli admin autonomy flush --lane trial-g4-autonomy --state queued`
- Preview updated budgets without persisting: `arw-cli admin autonomy budgets --lane trial-g4-autonomy --wall-clock-secs 600 --tokens 25000 --dry-run`
- Apply new budgets with spend guardrails: `arw-cli admin autonomy budgets --lane trial-g4-autonomy --wall-clock-secs 900 --tokens 30000 --spend-cents 1500 --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN`

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
