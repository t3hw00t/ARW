# CLI Reference
Updated: 2025-09-30
Type: Reference

Microsummary: Commands, subcommands, and flags for `arw-cli` with pointers to tutorials. Beta.

- Install: built with the workspace; see `just dev` and `nix develop` in `README.md`.
- Common flows: see Tutorials (Quickstart) and How‑to pages.

Commands (summary)
- `arw-cli` — prints version, hello, and effective paths
- `arw-cli paths [--pretty]` — effective runtime/cache/logs paths
- `arw-cli tools [--pretty]` — list registered tools
- `arw-cli gate keys [--doc|--details|--json {--pretty}]` — list known gating keys or render docs
- `arw-cli gate config schema [--pretty]` — print gating policy JSON schema
- `arw-cli gate config doc` — render the gating policy reference (Markdown)
- `arw-cli capsule template [--pretty|--compact]` — print a minimal capsule template
- `arw-cli capsule gen-ed25519 [--issuer <name>] [--out-pub <file>] [--out-priv <file>]` — generate keys
- `arw-cli capsule sign-ed25519 <sk_b64> <capsule.json> [--out <file>]` — sign capsule
- `arw-cli capsule verify-ed25519 <pk_b64> <capsule.json> <sig_b64>` — verify signature
- `arw-cli capsule status [--json] [--limit N]` — inspect active capsules
- `arw-cli capsule teardown [--id ID] [--all] [--reason TEXT] [--dry-run]` — revoke capsules via the emergency teardown API
- `arw-cli screenshots backfill-ocr [--lang <code>] [--force] [--dry-run]` — regenerate OCR sidecars via `/admin/tools/run`

See the [CLI Guide](../guide/cli.md) for examples. Use `--help` on any command for details.

Companion (TypeScript client CLI)
- After publishing `@arw/client`, a small Node-based CLI `arw-events` is available for tailing the SSE stream with resume and filters.
  - Install: `npm i -g @arw/client`
  - Usage:
    - `BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=$ARW_ADMIN_TOKEN arw-events --prefix service.,state.read.model.patch --replay 25`
  - Stores `Last-Event-ID` when `--store` is provided (default `.arw/last-event-id`).
