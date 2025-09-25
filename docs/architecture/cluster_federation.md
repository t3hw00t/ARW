---
title: Federated Clustering
---

# Federated Clustering

Updated: 2025-09-21
Type: Explanation

Clustering, live sharing, pooled compute, and optional revenue sharing let ARW grow beyond a single machine without losing the solo fast path. This page merges the earlier “Clustering” and “Hierarchy” drafts so the active plan is tracked in one place.

> **Status:** opt-in preview. Interfaces may change between releases; expect sharp edges while the shared sidecar, cluster matrix, and contribution ledger land.

## High-Level Shape
- **Home Node (Control Plane)**: owns projects, policies, budgets, provenance; there is always a single authoritative Home per workspace.
- **Workers (Compute Plane)**: advertise capabilities (models, GPUs, tools), accept jobs, stream events back.
- **Optional Broker (Discovery/Relay)**: tiny directory + relay for NAT cases; replaceable and optional when peers can connect directly.

Three planes keep responsibilities clean:
- **Control Plane** — scheduling, policies/leases, experiments, snapshots, ledger (lives on Home).
- **Compute Plane** — capability ads + job acceptance from Workers (local or remote).
- **Data Plane** — content-addressed artifacts (models, caches, world diffs) replicate on demand with strict egress rules.

## Roles & Negotiation
Hierarchy gives the cluster a vocabulary:
- Roles: `Root`, `Regional`, `Edge`, `Connector`, `Observer`.
- Topology: a DAG where edges represent delegation; non-exclusive by default.
- Epoch: monotonic counter incremented on topology changes to resolve conflicts.

Protocol types (lived in `arw-protocol` already):
- `CoreHello(id, role, capabilities, scope_tags, epoch, nonce)`
- `CoreOffer(from_id, proposed_role, parent_hint, shard_ranges, capacity_hint)`
- `CoreAccept(child_id, parent_id, role, epoch)`

Status:
- Legacy `arw-svc` exposed `/hierarchy/*`; the unified server now ships `/admin/hierarchy/state`, `/admin/hierarchy/role`, and the negotiation helpers (`hello`, `offer`, `accept`) as experimental HTTP endpoints.
- AsyncAPI topics `hierarchy.role.changed` and `hierarchy.state` continue to stream updates from both servers; cluster matrix views still land in the upcoming read-model work.

## Policy Capsules & Gating
Every interaction can carry a `GatingCapsule` via the `X-ARW-Capsule` header. Capsules propagate immutable denies, role/node/tags/time-window restrictions, and auto-renew leases. Ingress and egress guards enforce policy at the beginning and end of action/memory stacks (for example, `io:ingress:task.kind`, `io:egress:task.kind`). Regulatory Provenance Units (RPUs) add signatures, ABAC policy (Cedar), hop TTL, and adoption ledgers as part of the Complexity Collapse program. Legacy `X-ARW-Gate` headers are rejected to avoid silent fallbacks during federation rollout and emit failure telemetry for incident review.

## Configuration (Today)
`configs/default.toml` retains the transitional toggles while we finish porting:

```toml
[cluster]
enabled = false
bus = "local"    # "nats" when feature "arw-core/nats" is enabled
queue = "local"  # "nats" when feature "arw-core/nats" is enabled
# nats_url = "nats://127.0.0.1:4222"
```

Set `enabled = true` in your override config to join the preview. Export `ARW_EGRESS_PROXY_ENABLE=1` and `ARW_EGRESS_LEDGER_ENABLE=1` so Guardrail Gateway previews and the ledger capture offloads during federation tests.

The unified server already speaks NATS when compiled with the feature; JetStream durable queues land once the worker orchestration is active.

## Scheduling & Backpressure
- Jobs are idempotent with dedupe keys; results/logs stream as events so Home can reassign if a stream drops.
- Scheduler prefers local execution unless SLO/quality/budget demands offload; policy and egress leases travel with the job.
- Per-node queues and budgets prevent starved interactive sessions; long jobs fall back to batch lanes.

## Sharing Models and Worlds
- Models live in content-addressed storage (`state/models/by-hash/<sha>`). Workers announce hashes; Home instructs where to fetch and verifies by hash.
- Worlds (beliefs) replicate via filtered diffs with provenance and classification gates.
- Derived caches/embeddings export only policy-allowed material.

## Live Remote Sharing
- Shared sessions keep Episodes anchored on the Home node; guests connect with roles (`view`, `suggest`, `approve`, `drive`).
- Stream relay mirrors the live timeline (tokens, tool I/O, policy prompts). Control hand-offs are explicit and logged.
- Staging areas stay local; remote guests can propose risky actions but leases on Home approve them.

## Monetization & Cost Sharing
Contribution meters track accepted work per node (token usage, GPU-seconds, tool minutes, egress bytes). Revenue ledgers map income to Episodes and split by contribution weights. Initial settlements are manual CSV exports; no micro-payouts on the hot path.

## Observability
- Home materializes the full Episode timeline by merging remote events; guests see the same truth.
- Cluster Matrix dashboard (landing soon) shows nodes, health, queues, throughput, cost.
- Experiments annotate results by execution target (local vs specific remote).
- Model ads include `{count, preview_hashes[]}` so peers can request CAS blobs via gated admin routes.

## Unified Coverage
| Capability | Unified `arw-server` status |
| --- | --- |
| `/hierarchy/role`, `/hierarchy/state` | yes (HTTP, experimental) |
| `/hierarchy/hello\|offer\|accept` | yes (HTTP, experimental) |
| `/actions` queue (`/triad`) | yes; replaces the retired debug queue |
| Local bus ↔ NATS aggregator | yes (`arw-core/nats` feature) |
| gRPC control plane | planned (track in Roadmap) |
| Worker ledger & contribution meter | planned; ledger schema lives in kernel |
| Live guest session routing | porting to shared sidecar |


## MVP Path (Solo-Friendly)
1. Remote runner (one extra box) registers as Worker, accepts jobs, streams back results. Policies enforced on Home.
2. Cluster matrix shows both nodes; scheduler routes background work to remote nodes with per-node queues.
3. Live session sharing invites guests with explicit roles. Home keeps staging approvals.
4. Egress ledger + previews show what leaves, where, why, and cost before offload; ledger keeps audit trail.
5. Content-addressed models replicate by hash; Workers do not download without matching policies.
6. World diffs export policy-approved beliefs; imports track provenance and conflicts.
7. Contribution meter + revenue ledger produce a settlement report (CSV with math).
8. Optional broker handles NAT-straddling cases; remain stateless and easily replaceable.

## What to Keep Out (For Now)
- No multi-master control planes; avoid consensus in the hot path.
- No global shared memory by default; share views/diffs with explicit policies.
- No plugin auto-install from peers; artifacts stay signed and reviewed.
- No complex payment rails yet; keep to ledger exports.

## Acceptance Checks
- Invite a remote node, see it in the cluster matrix, offload a job with an egress preview, and watch the Episode timeline stitch together live.
- Killing a Worker mid-run does not corrupt the Episode; Home reassigns or fails cleanly with a replayable snapshot.
- Guests can co-drive under leases; risky actions always stage for approval with evidence.
- Monthly ledger shows contributions, offload costs, and final splits without extra spreadsheets.
- Turning clustering off leaves the single-node fast path untouched.

## Performance & Overhead
New costs are additive and only apply when federation is enabled. Local runs stay fast:
- Serialization/network hop only occurs when offloading; local worker remains default.
- Event journal appends are small; batch fsyncs keep overhead minimal compared to model latency.
- Policy checks are cached; egress redaction is streaming.
- Sampling keeps observability overhead predictable.
- Sandboxes reuse warm pools; heavy analysis stays off the hot path.

Guard overhead creep with budgets, SLOs, CI acceptance checks, and kill switches for clustering/experiments/tracing. Keeping the fast path intact ensures a small fixed cost in exchange for safety, explainability, and federation.
