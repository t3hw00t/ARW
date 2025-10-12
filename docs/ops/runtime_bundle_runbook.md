---
title: Runtime Bundle Runbook
---

# Runtime Bundle Runbook
Updated: 2025-10-12
Type: Runbook

Microsummary: Operational playbook for keeping managed runtime bundles signed, up to date, and recoverable.

## Update Cadence

### Channels
- **Preview** — weekly cuts every Wednesday (UTC). Ships the latest llama.cpp / whisper.cpp builds with regression test coverage and QA sign-off, then tags bundle catalogs under `channel: "preview"`.
- **Stable** — monthly release on the first Tuesday. Rolls forward all preview changes that survived the week-long soak, backports critical fixes, and refreshes documentation links.

### Release Steps
1. Regenerate bundle metadata and artifact manifests.
2. Verify staging artifacts locally:
   ```bash
   arw-cli runtime bundles manifest verify dist/bundles/<bundle-id>/bundle.json
   ```
   Remote snapshots (`arw-cli runtime bundles list --remote --pretty` or `GET /state/runtime/bundles`) now surface `signature.ok` plus warnings, so you can double-check each node after rollout without copying manifests back down.
3. Optional: run a local signature sweep before staging installs:
   ```bash
   arw-cli runtime bundles audit --require-signed
   ```
   Use `--dest <path>` when auditing a packaging workspace or exported bundle cache.
   Need to spot-check a running server instead? `arw-cli runtime bundles audit --remote --base https://hub.example.com --require-signed` will use `/state/runtime/bundles` and surface the server’s `signature_summary`.
   CI helper? Invoke `scripts/verify_bundle_signatures.sh` with `BASE_URL=https://hub.example.com` (and `ARW_ADMIN_TOKEN`) to run the same audit non-interactively—its exit code is suitable for pipelines.
4. Sign the manifest with the publishing key (CI runs the same command in headless mode):
   ```bash
   arw-cli runtime bundles manifest sign \
     dist/bundles/<bundle-id>/bundle.json \
     --key-file ops/keys/runtime_bundle_ed25519.sk \
     --issuer bundle-ci@arw \
     --key-id preview-bundle-signing
   ```
   Keep the public half of every active signing key in [`configs/runtime/bundle_signers.json`](../../configs/runtime/bundle_signers.json). The server and CLI load this registry automatically: `runtime bundles list` and `runtime bundles audit` now surface `trusted`/`untrusted` labels alongside signature health, and manifests signed by unknown keys are reported as failures when `--require-signed` (or `ARW_REQUIRE_SIGNED_BUNDLES=1`) is in effect.
5. Publish artifacts + signed manifest to the bundle registry and update the matching `configs/runtime/bundles.*.json` catalog entry (URL + `sha256`).
6. Notify operators (Launcher banner + `ops/runtime_bundle_runbook.md` change log) and document the new revision in the release notes.

Preview and stable channels share the same signing keys, but each channel increments its own revision history. Rotate the ed25519 pair quarterly (see `docs/ops/cluster_runbook.md` for key rotation flow) and keep the public key checked into `configs/runtime/bundle_signers.json` for automated verification.

### Enforcement Modes

- Set `ARW_REQUIRE_SIGNED_BUNDLES=1` on production servers to block unsigned or mismatched manifests during bundle reloads. When enabled, `runtime bundles reload` (CLI) and `/admin/runtime/bundles/reload` (remote) return an error until every installation passes signature validation.
- `runtime bundles list --remote --json` includes `signature_summary.enforced=true` once the guard is active so dashboards can surface a clear "signature enforcement" badge; the summary now also carries `trusted`/`rejected` counts so observability panels can highlight untrusted signatures even when at least one trusted signer is present.

## Rollback Checklist

1. **Inspect state** – capture the current installation root and available revisions.
   ```bash
   arw-cli runtime bundles list --pretty
   arw-cli runtime bundles rollback --bundle llama.cpp-preview/linux-x86_64-cpu --list
   ```
2. **Verify manifest integrity** – ensure the active revision is signed before continuing.
   ```bash
   arw-cli runtime bundles manifest verify \
     ~/.cache/arw/runtime/bundles/llama.cpp-preview/linux-x86_64-cpu/bundle.json
   ```
3. **Snapshot current files** – the rollback command auto-snapshots, but capture an explicit history entry if manual edits were made.
   ```bash
   arw-cli runtime bundles rollback \
     --bundle llama.cpp-preview/linux-x86_64-cpu \
     --revision rev-20251009T231500Z \
     --dry-run
   ```
   Review the summary, then rerun without `--dry-run` to apply.
4. **Validate post-rollback** – list bundles again, confirm the target revision is active, and re-run signature verification.
   ```bash
   arw-cli runtime bundles list --pretty
   arw-cli runtime bundles manifest verify \
     ~/.cache/arw/runtime/bundles/llama.cpp-preview/linux-x86_64-cpu/bundle.json
   ```
5. **Smoke the runtime** – trigger `just runtime-smoke` (or the channel-specific smoke) to confirm health, then log the rollback in the incident/register notes.

Keep at least the five most recent revisions on disk for fast restores. The CLI automatically prunes older entries when disk pressure warnings trigger, so export a copy to long-term storage after major releases.
