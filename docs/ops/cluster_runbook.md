---
title: Cluster Runbook
---

# Cluster Runbook

Updated: 2025-10-16
Type: Runbook

This runbook walks through bringing remote workers into an Agent Hub (ARW) installation and operating the preview federation stack. It assumes you already run the unified server (`arw-server`) locally and want to offload work or invite peers under policy control.

The cluster surfaces remain **preview**: APIs are admin-gated, subject to change, and default to a single-node fast path unless you explicitly opt in.

## Roles & Vocabulary
- **Home (Control Plane)** – authoritative node that owns projects, policies, budgets, and the event journal. Every workspace has exactly one Home.
- **Worker (Compute Plane)** – node that advertises capabilities (models, GPU/CPU availability) and accepts jobs from the Home.
- **Broker (Optional)** – NATS or custom relay that offers discovery when nodes cannot reach each other directly.
- **Capsules & Leases** – policy envelopes (`X-ARW-Capsule` headers) that gate what a worker may execute. See `docs/architecture/asimov_capsule_guard.md`.
- **Cluster Topics** – events like `cluster.node.advertise` and `cluster.node.changed` broadcast node state; subscribe via SSE for live dashboards.

## Prerequisites
- Unified server version `0.2.0-dev` (or newer) on all nodes. Build from the same commit or archived tag when replaying older snapshots.
- Stable network path between Home and Workers (direct TCP on `8091` or via reverse proxy). Optional NATS broker reachable if you plan to use shared queues/events.
- Strong admin token exported on each node: `export ARW_ADMIN_TOKEN=$(openssl rand -hex 32)`.
- Config overrides writable on each machine (for example `configs/default.toml` beside the binaries or `${ARW_STATE_DIR}/config.toml`).
- Optional: managed runtime bundles installed if you expect workers to host llama.cpp or vision runtimes.

## Configuration Overview

1. **Enable federation on each node** by adding to your config:

```toml
[cluster]
enabled = true
# Optional:
# node_id = "my-worker" # defaults to hostname.
# bus = "nats"          # keep "local" when no broker is present.
# queue = "nats"
# nats_url = "nats://127.0.0.1:4222"
```

Set the node role via the hierarchy admin API instead of config files:

```bash
curl -sS -X POST http://127.0.0.1:8091/admin/hierarchy/role \
  -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"role":"home"}'
```

2. **Set stable identifiers** using environment variables if you prefer:

```bash
export ARW_NODE_ID=home-east    # or worker-gpu-01, etc.
export ARW_EGRESS_PROXY_ENABLE=1
export ARW_EGRESS_LEDGER_ENABLE=1
```

3. **Admin credentials** – all cluster endpoints live under `/admin/**` and require `Authorization: Bearer <token>` or the legacy `ARW_ADMIN_TOKEN` header.

4. **Optional broker** – when using NATS, compile bridge services with the feature flag and run them alongside the server:

```bash
cargo build -p arw-connector --features nats --release
ARW_NODE_ID=connector-01 \
ARW_NATS_URL=nats://broker.yourdomain:4222 \
ARW_GROUP=workers \
./target/release/arw-connector
```

The connector relays local task completions into cluster-wide subjects (`arw.events.task.completed`) and mirrors node-specific topics. Additional workers can connect directly to the broker when `bus="nats"` and `queue="nats"` are enabled.

## Bring Up Sequence

### 1. Home Node
1. Update config (`cluster.enabled = true`) and restart `arw-server`.
2. Verify health:
   ```bash
   curl -sS http://127.0.0.1:8091/healthz
   ```
3. Set your desired hierarchy role (optional; defaults to root/edge). Example:
   ```bash
   curl -sS -X POST http://127.0.0.1:8091/admin/hierarchy/role \
     -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"role":"root"}'
   ```
4. Confirm state:
   ```bash
   curl -sS http://127.0.0.1:8091/admin/hierarchy/state \
     -H "Authorization: Bearer $ARW_ADMIN_TOKEN" | jq
   ```
5. Snapshot cluster registry:
   ```bash
   curl -sS http://127.0.0.1:8091/state/cluster \
     -H "Authorization: Bearer $ARW_ADMIN_TOKEN" | jq
   ```

The Home advertises itself every five minutes and whenever runtime/models/governor state changes. Watch for `cluster.node.advertise` SSE events to confirm broadcasts.

### 2. Worker Node
1. Install the same build (`arw-server`, `arw-cli`, runtimes). Export consistent identifiers:
   ```bash
   export ARW_NODE_ID=worker-gpu-01
   export ARW_ADMIN_TOKEN=<strong-token>
   ```
2. Update the worker config to point at the Home. When running behind a secure tunnel, route HTTP requests through the tunnel while keeping `ARW_BIND=127.0.0.1`.
3. Start the server and set its role to `worker` (edge) or whichever tier you want to reflect:
   ```bash
   curl -sS -X POST http://127.0.0.1:8091/admin/hierarchy/role \
     -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{"role":"edge"}'
   ```
4. If using capsules or leases, install them before accepting work:
   ```bash
   arw-cli capsule status --base http://127.0.0.1:8091 \
     --admin-token "$ARW_ADMIN_TOKEN"
   ```
5. Configure the worker’s self-identity (on the worker itself). This sets the ID, role, and capability tags the local server will advertise over the event bus:
   ```bash
   curl -sS -X POST http://127.0.0.1:8091/admin/hierarchy/hello \
     -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
     -H "Content-Type: application/json" \
     -d '{
       "id": "worker-gpu-01",
       "role": "edge",
       "capabilities": ["gpu:rtx-4090","memory:96g"],
       "scope_tags": ["gpu","balanced"],
       "epoch": 0,
       "nonce": "b75d4d48-3df2-4e1e-8fab-12efab809d4b"
     }'
   ```
   Behind the scenes the worker publishes `cluster.node.advertise` events on its local bus. To reach the Home you need either:
   - A shared NATS bus (`bus="nats"`, `queue="nats"`) so advertisements fan out automatically.
   - A relay such as `arw-connector` that forwards cluster events between nodes.
   Without a shared bus the Home will not see remote nodes yet.
   Use any monotonic `epoch` counter (start at `0`) and generate a fresh `nonce` per hello (for example `uuidgen`).
6. Confirm the worker appears on the Home once events propagate:
   ```bash
   curl -sS https://home.example.com/state/cluster \
     -H "Authorization: Bearer $HOME_ADMIN_TOKEN" | jq '.nodes[] | select(.id=="worker-gpu-01")'
   ```

### 3. Routing Jobs
- The scheduler prefers local execution until latency or policy hints suggest offloading. Set hints via the governor (`/admin/governor`) or per-job leases once exposed.
- Monitor `/state/actions` (filter by `state=running`) to see which node owns a run. Remote completions emit `task.completed` events with the worker’s node ID.
- Use the runtime matrix (`/state/runtime_matrix`) to confirm worker runtimes report healthy restart budgets and accessible status strings.

## Observability & Telemetry
- **Cluster snapshot** – `/state/cluster` returns `{nodes[], generated, generated_ms}`. Each `ClusterNode` includes id, role, optional health, and advertised capabilities.
- **CLI shortcut** – `arw-cli state cluster --base http://127.0.0.1:8091` renders a table of nodes with last-seen timestamps and stale markers; add `--json --pretty` for raw output.
- **Runtime bundles** – `/state/runtime_matrix` now includes `signature_summary.trust_shortfall`: manifests with valid signatures but no matching signer registry entry per channel. Install or update `configs/runtime/bundle_signers.json` (or set `ARW_RUNTIME_BUNDLE_SIGNERS`) so preview bundles graduate to fully trusted status.
- **Events** – subscribe to `/events?prefix=cluster.` or `/events?prefix=hierarchy.` for live updates.
- **Logs** – enable structured logs with `ARW_LOG=info` (default) and tail worker logs for connectivity issues.
- **Contribution metrics (preview)** – ledger endpoints under `/state/egress` and `/state/runtime_matrix` will surface per-node contribution once Phase 2 lands.

## Troubleshooting
- **Unauthorized** – ensure `Authorization: Bearer <token>` matches `ARW_ADMIN_TOKEN`. The server refuses cluster endpoints without it.
- **Node missing in snapshot** – worker may not have re-advertised yet; ensure a shared bus is configured and force a refresh by hitting the worker’s `/admin/hierarchy/hello` again or restarting it.
- **Event bus mismatch** – both Home and Worker must agree on `bus`/`queue` settings (`local` vs `nats`). Mixed modes suppress remote broadcasts.
- **NATS connection loop** – confirm `arw-connector` and worker servers were compiled with `--features arw-core/nats`. The feature gate is compile-time.
- **Capsule denies** – inspect `/state/policy/capsules` and recent `policy.capsule.*` events. Remove or widen leases before retrying.
- **Firewall** – workers contacting Home must reach `/admin/hierarchy/*` and `/actions` endpoints. Confirm proxies preserve `Authorization` headers.

## Related References
- `docs/architecture/cluster_federation.md` – deeper protocol and roadmap details.
- `docs/guide/runtime_matrix.md` – understanding runtime health propagation.
- `docs/architecture/capability_consent_ledger.md` – policy and lease vocabulary.
- `docs/guide/security_posture.md` – guidance for binding and reverse proxies.
- `docs/CONFIGURATION.md` – full list of `ARW_*` environment variables.
