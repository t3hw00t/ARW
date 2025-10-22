---
title: Runtime Manager Operations Guide
---

# Runtime Manager Operations Guide
Updated: 2025-10-22
Type: Tutorial

This guide explains how to operate the managed runtime supervisor end‑to‑end: discovering bundle catalogs, installing or importing runtime payloads (online and offline), satisfying consent checkpoints, and launching or shutting down managed runtimes. It complements the higher-level [Runtime Quickstart](runtime_quickstart.md) by focusing on the CLI surfaces that ship with the modular runtime stack.

## Prerequisites

- A working ARW installation with the managed runtime supervisor enabled (`apps/arw-server` ≥ the current main branch).
- `arw-cli` available in your shell (`cargo install --locked arw-cli` or use the repo workspace).
- An admin token with the `runtime:manage` capability. Obtain it via your policy process (see [Gating Keys](../GATING_KEYS.md)); if you self-host, create one with:
  ```bash
  # Persist an admin token locally (writes to configs/security/.env by default)
  arw-cli admin token persist --path configs/security/.env --print-token
  
  # Request a lease that includes runtime:manage (adjust scope + TTL as needed)
  ARW_ADMIN_TOKEN=<token> \
    arw-cli admin scope lease request \
      --scope operators@local \
      --lease-cap runtime:manage \
      --ttl-secs 7200
  ```
- Hugging Face or other model artifact credentials when installing official bundles that cite remote URLs.
- Enough disk space under `<state_dir>/runtime/bundles` (default staging area for managed runtimes).

Set the admin token for CLI use (either export `ARW_ADMIN_TOKEN` or pass `--admin-token` on each command).

## 1. Inspect the Supervisor Snapshot

Use the runtime status subcommand to confirm that the supervisor is reachable and to review active bundles, restart budgets, and health summaries:

```bash
arw-cli runtime status
# or JSON for automation:
arw-cli runtime status --json | jq
```

Key fields:
- `state` / `state_label`: launch state reported by the supervisor (`ready`, `starting`, `error`, etc.).
- `restart_budget`: remaining automatic restarts and the next reset time.
- `severity` / `summary`: aggregated health signal that also drives Launcher toasts.

If the command fails with `runtime:manage lease required`, double-check your capability grant and reissue the lease request.

## 2. Discover Bundle Catalogs

The supervisor reads bundle metadata from the repository (`configs/runtime/*.json`) and from any bundles staged under `<state_dir>/runtime/bundles`. Catalogue them via the CLI:

```bash
# Local catalog snapshot
arw-cli runtime bundles list

# Ask a running server for its current view
arw-cli runtime bundles list --remote --base http://127.0.0.1:8091

# Pretty JSON for scripting or inspection
arw-cli runtime bundles list --json --pretty
```

Each bundle entry includes:
- `id`: canonical identifier (e.g., `llama.cpp-preview/linux-x86_64-cpu`).
- `profiles` and `modalities`: hints for matching workloads.
- `metadata.consent`: consent overlays required before enabling the runtime (audio/vision bundles include pointers to ledger entries and UI overlays).

## 3. Refresh Catalogs

When you update `configs/runtime` or deploy new bundle files, tell the supervisor to rescan:

```bash
arw-cli runtime bundles reload --base http://127.0.0.1:8091
```

This command is idempotent; it asks the server to reload bundle manifests without touching staged artifacts.

## 4. Install Bundles (Online)

To download managed bundles (artifacts + manifest) directly from the catalog:

```bash
# Install two bundles into the default state directory
arw-cli runtime bundles install \
  llama.cpp-preview/linux-x86_64-cpu \
  llama.cpp-preview/windows-x86_64-directml

# Stage into a custom directory (useful for review or portable media)
arw-cli runtime bundles install \
  llama.cpp-preview/linux-x86_64-cpu \
  --dest /tmp/runtime-bundles

# Filter by artifact metadata (kind/format) to limit downloads
arw-cli runtime bundles install \
  llama.cpp-preview/linux-x86_64-cpu \
  --artifact-kind model --artifact-kind binary
```

Flags to remember:
- `--remote`: read catalogs from the server instead of local files.
- `--dry-run`: show planned actions without downloading.
- `--force`: overwrite existing artifacts or metadata (also snapshots the previous revision before replacing files).
- `--dest DIR`: set an alternate staging root; copy it to `<state_dir>/runtime/bundles/` afterwards for activation.

During install the CLI writes `bundle.json` metadata alongside the artifacts, recording consent annotations and source provenance. Review it to ensure required overlays are in place:

```bash
cat <state>/runtime/bundles/llama.cpp-preview__linux-x86_64-cpu/bundle.json | jq '.metadata.consent'
```

## 5. Air-Gapped / Offline Imports

When the target machine cannot reach bundle URLs:

1. On a connected host, install bundles into a staging directory:
   ```bash
   arw-cli runtime bundles install \
     llama.cpp-preview/linux-x86_64-cpu \
     --dest /mnt/staging/runtime-bundles
   ```
   Copy the resulting `<bundle_dir>` (including `bundle.json` and `artifacts/`) onto removable media.

2. On the offline host, import the payload:
   ```bash
   arw-cli runtime bundles import \
     --bundle llama.cpp-preview/linux-x86_64-cpu \
     --metadata /media/usb/runtime-bundles/llama.cpp-preview__linux-x86_64-cpu/bundle.json \
     /media/usb/runtime-bundles/llama.cpp-preview__linux-x86_64-cpu/artifacts
   ```

   - Provide multiple paths when artifacts are split across directories.
   - Use `--force` to replace an existing bundle (a snapshot is stored before overwrite).
   - Add `--dry-run` to validate directory layout without copying.

3. Re-run `arw-cli runtime bundles list` to confirm the bundle is now available locally.

Bundles with missing URLs (e.g., private mirrors) emit a hint during `install`; stage those manually with `runtime bundles import`.

## 6. Verify Signatures & Consent

Trusted deployments require bundle signatures and consent metadata:

```bash
# Check that installed bundles have valid signatures and required consent fields
arw-cli runtime bundles audit

# List bundles with signatures that couldn't be matched to a trusted signer
arw-cli runtime bundles trust-shortfall

# Sign or verify manifests (ed25519)
arw-cli runtime bundles manifest sign bundle.json --key-file secret.key
arw-cli runtime bundles manifest verify bundle.json --require-trusted
```

Common follow-ups:
- Update `ARW_RUNTIME_BUNDLE_SIGNERS` with your signer registry before running `audit`.
- Use `--json` or `--pretty` on `audit` to feed CI.
- Keep the consent ledger aligned: if a bundle introduces new sensors, update the consent overlay referenced in `metadata.consent`.

## 7. Launch and Shutdown Runtimes

Once bundles are staged and verified:

```bash
# Request a restore (launch) for the named runtime
arw-cli runtime restore --runtime llama.cpp-preview/linux-x86_64-cpu

# Retrieve live status again to ensure it reached ready
arw-cli runtime status --runtime llama.cpp-preview/linux-x86_64-cpu

# Shut down when maintenance is required
arw-cli runtime shutdown --runtime llama.cpp-preview/linux-x86_64-cpu
```

The restore API honours consent annotations:
- If a bundle requires explicit consent, Launcher surfaces the overlay and records the acceptance; the CLI reflects the status in `runtime status` under `consent.state`.
- Attempts to launch without consent emit `policy_decision` events; subscribe via `arw-cli events modular --follow` during testing.

Restart budgets and cooldowns are enforced automatically; the CLI reports remaining attempts. To investigate repeated failures, tail the supervisor events:

```bash
arw-cli events modular --prefix runtime.state.changed --follow
```

## 8. Troubleshooting Checklist

- **Supervisor unavailable**: ensure the server is running and your admin token grants `runtime:manage`.
- **Missing bundles**: re-run `arw-cli runtime bundles reload` and verify catalog JSON under `configs/runtime/`.
- **Signature failures**: confirm the signer registry includes the key id embedded in `bundle.json`. Re-sign with `runtime bundles manifest sign`.
- **Consent warnings**: update `metadata.consent` in the catalog or attach the latest ledger identifier; Launcher will continue blocking launches until the consent state is satisfied.
- **Offline copies stale**: compare the `installed_at` timestamp in `bundle.json`; rerun `import` with `--force` after copying updated artifacts.

## 9. Related References

- [Managed Runtime Supervisor Blueprint](../architecture/managed_runtime_supervisor.md)
- [Managed llama.cpp Runtime Blueprint](../architecture/managed_llamacpp_runtime.md)
- [Runtime Quickstart (Non-Technical)](runtime_quickstart.md)
- [Vision Runtime Guide](vision_runtime.md)
- [Gating Keys](../GATING_KEYS.md) (`runtime:manage` policy details)

Use this guide as the operational runbook for staging new runtimes or supporting air-gapped deployments. Keep it alongside your consent ledger and bundle signer registry so new operators can walk through the same workflow without regressions.
