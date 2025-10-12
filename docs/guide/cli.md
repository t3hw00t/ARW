---
title: CLI Guide
---

# CLI Guide
Updated: 2025-10-12
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
- HTTP fetch
  - `arw-cli http fetch https://example.com --wait-timeout-secs 45` — submits `net.http.get`, waits for completion, and prints status/headers plus a UTF-8 preview (decoded from the built-in `http.fetch` head capture). Requires an active `net:http` or `io:egress` lease.
  - `--header "User-Agent: MyBot/1.0"` (repeatable) adds request headers; `--method post --data '{"q":"search"}' --content-type application/json` sends a JSON body; `--preview-kb 128` enlarges the streamed preview head (1–1024 KB) per request; `--connector-id search-searxng` routes through the optional SearXNG metasearch proxy.
  - `--output page.html` writes the preview bytes to disk (full body when small; otherwise the truncated head). Use `--raw-preview` to print base64 instead of attempting UTF-8 decoding.
  - Pair with `arw-cli gate lease` or `/leases` helpers to grant temporary `net:http` access when the workspace posture requires it.
- Managed runtime bundles
  - Local catalogs: `arw-cli runtime bundles list --dir ./configs/runtime --pretty`
  - Alternate install root: `arw-cli runtime bundles list --install-dir ~/.cache/arw/bundles`
  - Remote snapshot (catalogs + installed bundles): `arw-cli runtime bundles list --remote --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN`
  - Trigger rescan: `arw-cli runtime bundles reload --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN`
  - Both local and remote listings surface a `Signatures:` line summarizing totals, failures, and whether enforcement is active
  - Start a managed runtime: `arw-cli runtime restore llama.cpp-preview/linux-x86_64-cpu --no-restart`
  - Stop a managed runtime: `arw-cli runtime shutdown llama.cpp-preview/linux-x86_64-cpu`
  - Download artifacts: `arw-cli runtime bundles install llama.cpp-preview/linux-x86_64-cpu`
  - Offline import: `arw-cli runtime bundles import --bundle llama.cpp-preview/linux-x86_64-cpu ./llama-build`
  - Roll back to a previous snapshot: `arw-cli runtime bundles rollback --bundle llama.cpp-preview/linux-x86_64-cpu --list`
  - Verify manifest signatures: `arw-cli runtime bundles manifest verify ~/.cache/arw/runtime/bundles/llama.cpp-preview/linux-x86_64-cpu/bundle.json`
  - Sign manifests before publishing: `arw-cli runtime bundles manifest sign dist/bundles/llama.cpp-preview/linux-x86_64-cpu/bundle.json --key-file ops/keys/runtime_bundle_ed25519.sk --issuer bundle-ci`
  - Add `--json`/`--pretty` to the rollback command for machine-readable history listings or outcome summaries
  - `runtime bundles list --pretty` includes signature verification results for each installed bundle, including aggregated `trusted`/`rejected` counts and per-key `[trusted]`/`[untrusted]` hints
  - Offline audit: `arw-cli runtime bundles audit --require-signed` fails fast when any installed bundle lacks a trusted manifest; add `--dest` to point at alternate roots
  - Remote audit: `arw-cli runtime bundles audit --remote --base http://hub:8091 --require-signed` checks the running server and surfaces both the enforcement flag and the trusted/untrusted totals exposed by `/state/runtime/bundles`
  - Production guardrail: set `ARW_REQUIRE_SIGNED_BUNDLES=1` so `runtime bundles reload` refuses unsigned manifests; `runtime bundles list --remote --json` will report `signature_summary.enforced:true`
- Automation helper: `BASE_URL=https://hub scripts/verify_bundle_signatures.sh` wraps the remote audit and exits non-zero when signatures are missing—drop it into CI jobs.
- Prefer `just verify-signatures --base https://hub --token $ARW_ADMIN_TOKEN` for local checks; it delegates to the same script.
- Preview installs: add `--dry-run` to either command; use `--dest /custom/path` when staging bundles outside the default `<state_dir>/runtime/bundles`
- Scripting: add `--json`/`--pretty` to either command; remote mode fetches `/state/runtime/bundles` (including `installations`) from the running server.

Logic Units
- Inspect local manifests: `arw-cli logic-units inspect examples/logic-units/retrieval-mmr-rrf.yaml --json` (works on single files or folders; validates against the schema before printing a summary)
- Install to the kernel: `arw-cli logic-units install examples/logic-units/memory-hygiene.yaml --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN` (pair with `--dry-run` to preview payloads and `--id` to override manifest ids at publish time)
- List registered units: `arw-cli logic-units list --base http://127.0.0.1:8091 --admin-token $ARW_ADMIN_TOKEN --json --pretty`
- Examples in `examples/logic-units/` stay schema-validated via `cargo test -p arw-cli logic_unit_examples_validate_against_schema`, so you can treat the gallery as a starting point for new packs.

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
- List server-managed presets
  - `arw-cli capsule preset list --base http://127.0.0.1:8091`
  - Local configs only: `arw-cli capsule preset list --local --json --pretty`
- Adopt a preset via the server
  - `arw-cli capsule preset adopt --id capsule.strict-egress --base http://127.0.0.1:8091 --reason "incident response" --show-status`
- Audit capsule events
  - `arw-cli capsule audit --base http://127.0.0.1:8091 --limit 25`
- Manage trust issuers
  - Inspect current entries: `arw-cli capsule trust list`
  - Rotate keys and reload trust store: `arw-cli capsule trust rotate --id local-admin --reload --base http://127.0.0.1:8091`
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
