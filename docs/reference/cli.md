# CLI Reference
Updated: 2025-10-12
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
- `arw-cli capsule preset list [--base URL] [--local] [--json {--pretty}]` — enumerate capsule presets served by `/admin/policy/capsules/presets` (or from configs/capsules when `--local`)
- `arw-cli capsule preset adopt --id ID [--base URL] [--reason TEXT] [--show-status]` — adopt a server-managed preset via `/admin/policy/capsules/adopt`
- `arw-cli capsule audit [--base URL] [--limit N] [--json {--pretty}]` — tail capsule adoption/teardown events through `/admin/policy/capsules/audit`
- `arw-cli capsule trust list|add|remove|rotate [...]` — inspect and rotate trusted capsule issuers (writes `configs/trust_capsules.json`; `rotate` can trigger `/admin/rpu/reload`)
- `arw-cli screenshots backfill-ocr [--lang <code>] [--force] [--dry-run]` — regenerate OCR sidecars via `/admin/tools/run`
- `arw-cli runtime bundles list [--dir PATH] [--install-dir PATH] [--remote] [--json {--pretty}]` — inspect managed runtime bundle catalogs and installed bundles locally or via `/state/runtime/bundles`
- `arw-cli runtime bundles reload [--json {--pretty}]` — trigger `/admin/runtime/bundles/reload` to rescan bundle catalogs on the server
- `arw-cli runtime bundles install [--dir PATH] [--remote] [--dest DIR] [--artifact-kind KIND] [--artifact-format FORMAT] [--force] [--dry-run] <bundle-id>...` — download bundle artifacts into the managed runtime directory (defaults to `<state_dir>/runtime/bundles`)
- `arw-cli runtime bundles import --bundle <id> [--dest DIR] [--metadata FILE] [--force] [--dry-run] <path>...` — copy local archives or directories into the managed runtime directory for offline installs
- `arw-cli runtime bundles rollback --bundle <id> [--dest DIR] [--revision REV] [--list] [--dry-run] [--json {--pretty}]` — list available revisions and restore a bundle from the local history snapshot (JSON mode works for both `--list` and rollback actions)
- `arw-cli runtime bundles manifest sign <manifest> [--key-b64 B64 | --key-file FILE] [--issuer NAME] [--key-id ID] [--output FILE] [--compact]` — append or replace ed25519 signature entries on a bundle manifest and emit canonical sha256 metadata
- `arw-cli runtime bundles manifest verify <manifest> [--json {--pretty}] [--require-trusted]` — validate manifest signatures, hashes, and key metadata (append `--require-trusted` to fail unless a signer registry entry matches)
- `arw-cli runtime bundles audit [--dest DIR | --remote --base URL] [--json {--pretty}] [--require-signed]` — scan installed bundles (defaults to `<state_dir>/runtime/bundles`) or a running server and report signature coverage, optionally failing when unsigned manifests are detected
- `arw-cli research-watcher list|approve|archive [...]` — inspect queue snapshots and bulk-approve/archive Suggested logic units; supports `--status`, `--limit`, `--ids`, `--from-status`, `--filter-source`, `--filter-contains`, `--note`, `--dry-run`, and JSON output coupled with the usual `--base`/`--admin-token` flags.
- Just shortcuts: `just research-watcher-list`, `just research-watcher-approve`, and `just research-watcher-archive` wrap the same helpers with defaults; pass overrides like `base=https://hub token=$ARW_ADMIN_TOKEN` or trailing ids (for approve/archive) as needed.
- Convenience wrapper: `scripts/verify_bundle_signatures.sh` runs the remote audit with `--require-signed`, respecting `BASE_URL`, `ARW_ADMIN_TOKEN`, and additional CLI flags—ideal for CI pipelines.
- Local and remote bundle snapshots now include per-installation `signature` blocks (`ok`, `canonical_sha256`, `warnings`, per-key status) and a top-level `signature_summary` (with `enforced` flag) so operators can spot unsigned or mismatched manifests at a glance and confirm when enforcement is active.

Human-readable output from `runtime bundles list` now includes consent summaries derived from each catalog’s `metadata.consent` block (for example, `consent: required (vision)` or `consent: missing metadata for audio/vision modalities`). Any audio/vision bundle without annotations is flagged so operators can update the catalog before promoting the runtime.
- `arw-cli runtime shutdown <id>` — request a managed runtime shutdown via `/orchestrator/runtimes/{id}/shutdown`
- `arw-cli admin autonomy lanes [--json {--pretty}]` — list autonomy lanes with alert and budget summaries.
- `arw-cli admin autonomy lane --lane <id> [--json {--pretty}]` — inspect a specific lane for current mode, jobs, alerts, and budgets.
- `arw-cli admin autonomy pause --lane <id> [--operator NAME] [--reason TEXT]` — pause scheduling and running jobs under a lane.
- `arw-cli admin autonomy resume --lane <id> [--mode guided|autonomous] [--operator NAME] [--reason TEXT]` — resume a lane in guided or autonomous mode.
- `arw-cli admin autonomy stop --lane <id> [--operator NAME] [--reason TEXT]` — stop a lane and flush queued + in-flight jobs.
- `arw-cli admin autonomy flush --lane <id> [--state all|queued|in_flight]` — clear queued or running jobs without changing the lane mode.
- `arw-cli admin autonomy budgets --lane <id> [--wall-clock-secs N] [--tokens N] [--spend-cents N] [--clear] [--dry-run] [--json {--pretty}]` — preview or persist lane budgets.

See the [CLI Guide](../guide/cli.md) for examples. Use `--help` on any command for details.

Companion (TypeScript client CLI)
- After publishing `@arw/client`, a small Node-based CLI `arw-events` is available for tailing the SSE stream with resume and filters.
  - Install: `npm i -g @arw/client`
  - Usage:
    - `BASE=http://127.0.0.1:8091 ARW_ADMIN_TOKEN=$ARW_ADMIN_TOKEN arw-events --prefix service.,state.read.model.patch --replay 25`
  - Stores `Last-Event-ID` when `--store` is provided (default `.arw/last-event-id`).
