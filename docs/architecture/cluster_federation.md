---
title: Federated Clustering
---

# Federated Clustering

Clustering, live sharing, pooled compute, and optional revenue sharing fit ARW without replacing the single‑node design. Keep a single authoritative Home Node per workspace, invite Workers under strict policy and egress control, share models and “world” diffs (not raw secrets), and meter contributions.

This page outlines a solo‑maintainable blueprint and an MVP path. The default fast‑path (local, single node) remains essentially unchanged.

## High‑Level Shape
- Home Node (Control Plane): owns the project, policies, budgets, provenance; non‑multi‑master.
- Workers (Compute Plane): advertise capabilities (models, GPUs, tools), accept jobs, stream events back.
- Optional Broker (Discovery/Relay): tiny directory + relay to help NAT; replaceable; peers can connect directly if available.

## Three Planes
- Control Plane: scheduling, policies/leases, experiments (A/B), snapshots, ledger. Lives on Home Node.
- Compute Plane: capability ads + job acceptance from Workers; can be local or remote machines.
- Data Plane: content‑addressed artifacts (models, caches, snapshots, world diffs) replicate on demand with strict egress rules.

## Core Concepts (Federated, Not Replaced)
- Workspaces/Projects remain the tenancy boundary.
- Episodes remain the unit of work; correlate via `corr_id`.
- Runtime Matrix extends to Cluster Matrix: rows are (node, hardware, sandbox, model/tool availability, cost/latency SLOs, health).
- Policies gain egress rules (what may leave, to whom, how long) and leases (time‑boxed grants).
- World Model stays project‑scoped; share views/diffs with provenance, not raw secrets.

## Live Remote Sharing
- Shared session: Home hosts the episode; guests connect with roles (view/suggest/approve/drive).
- Stream relay: Home relays the live timeline (tokens, tool I/O, policy prompts). Control hand‑offs are explicit and logged.
- Staging Area remains local: risky actions queue at Home; remote guests can propose; you approve via a lease.

## Pooled Compute (Minimal Coordination)
- Node identity & heartbeat: Workers have stable IDs; publish capabilities + health; renew short‑TTL leases.
- Jobs: idempotent with dedupe keys; results/logs stream as events; Home can reassign on stream drop.
- Scheduling: prefer local; offload when SLO/quality/budget requires; respect data classification; apply egress leases per job.
- Backpressure: per‑node queues and budgets; preempt for interactive jobs; long jobs to batch lanes.

## Sharing Models and Worlds
- Models: content‑addressed artifacts (hash‑named packs). Workers announce what they have; Home instructs fetching from allowed peers/registry; verify by hash.
- Worlds (beliefs): share filtered diffs with provenance and confidence; conflicts surface for review. Classification governs exportable beliefs.
- Caches/embeddings: export derived features only per policy; raw sources private by default.

## Monetization and Cost Sharing
- Contribution meter: per node, track accepted work (token‑in/out, GPU‑sec, tool minutes, egress bytes).
- Revenue ledger: map income to episodes; split by contribution weights after costs; settle periodically (no live micro‑payouts initially).
- Policy gates: nodes may be “paid‑only”; projects may be “no‑egress”; scheduler matches jobs to policy+budget.

## Security and Trust
- Invite‑only federation: you approve nodes; each has a keypair; mTLS for transport; every request carries workspace+policy context.
- Signed manifests: tools/plugins/models declare capabilities and versions; Workers refuse unknown/unsigned payloads.
- Egress preview: before offload, show “what leaves, where, why, how much it costs,” and require a lease; log to an egress ledger.
- Deterministic replay: remote actions stub snapshots with provider/node IDs; replay locally (minus remote execution) for comparison.

## Observability
- Unified timeline: Home materializes the complete episode timeline by merging remote events; guests see the same truth.
- Cluster Matrix dashboard: nodes, health, queues, throughput, costs; keep it red/yellow/green.
- Experiment deltas: A/B results broken down by target (local vs specific remote).
- Model ads: nodes include a models summary `{count, preview_hashes[]}`; peers can request specific CAS blobs via a gated admin route.

## MVP Path (Solo‑Friendly)
1) Remote runner (one extra box): allow a second node to register as a Worker, accept jobs, and stream results back. Policies/budgets enforced at Home.
2) Cluster Matrix + scheduler: show both nodes; route simple offloads (long‑context inference, heavy tools); per‑node queues.
3) Live session sharing: “Invite guest” with roles (view/suggest/drive). Staging approvals remain on Home.
4) Egress ledger + previews: before offload, show payload summary + cost; record in ledger.
5) Content‑addressed models: Workers announce model hashes; Home instructs where to fetch; verify on load.
6) World diffs: enable export of “public beliefs” with provenance to invited nodes; review conflicts on import.
7) Contribution meter + revenue ledger: track contributions per node; add a settlement report (CSV + clear math).
8) Minimal broker (optional): tiny relay/directory for NAT‑tricky cases; stateless and replaceable.

## What To Keep Out (For Later)
- No multi‑master control planes (avoid consensus).
- No global shared memory by default; share views/diffs behind policies.
- No plugin auto‑install from peers; everything is signed, reviewed, and sandboxed.
- No complex payment rails initially; start with a ledger + manual settlement.

## Acceptance Checks
- Invite a remote node, see it in the Cluster Matrix, offload a job with an egress preview, and watch the episode timeline stitch together live.
- Killing a Worker mid‑run does not corrupt the episode; Home reassigns or fails cleanly with a replayable snapshot.
- A guest can co‑drive an agent, but risky actions always stage for your approval with evidence and a lease.
- The monthly ledger clearly shows contributions, offload costs, and final split (no side spreadsheets).
- Turning clustering off leaves a fully functional single‑node system (same UI, same APIs).

## Performance & Overhead (Fast‑Path Preserved)

You don’t “lose” meaningful optimization if you keep a fast path. New costs are additive and only apply when used. Local runs stay fast.

Where overhead appears and how to keep it small:
- RPC vs in‑process calls: pay serialization + a network hop only when offloading. Keep a local runner as default; ship artifacts by reference (content‑addressed blobs) and stream minimal deltas.
- Event journal + provenance: one small append per event; batch fsyncs; coarse‑grain high‑volume loops. Model/tool latency dwarfs this.
- Policy/egress checks: a fixed decision step with cached leases. Redaction runs as a streaming filter (single pass hashing/stripping).
- Observability/tracing: sample low‑value spans; aggregate counters and flush periodically; timeline is a materialized view over the same events.
- Sandboxes/containers: reuse warm sandboxes; use lightweight isolation; only containerize untrusted tools.
- Cluster coordination: short‑TTL heartbeats with backoff; sticky sessions or a tiny fan‑out relay; no heavy consensus.
- World model/belief graph: incremental updates; TTL/dedup; keep only exportable, evidence‑backed claims.
- Experiments (A/B): run on saved tasks or narrow slices; schedule in batch lanes; disable during interactive sessions unless requested.
- Verification: hash/sign once per artifact; cache results; content‑addressed storage prevents re‑verification.

Fast‑path protection rules:
- Local‑only mode: if no offload/experiments/tracing, overhead ≈ one policy check + one journal append.
- Warm pools: keep model runners, sandboxes, and caches warm per session.
- Speed‑run toggle: reduce tracing detail and disable nonessential background jobs for latency‑critical work.
- Keep heavy analysis off the hot path: compaction, index builds, long parsing run in batch lanes with budgets.

Guardrails so overhead never creeps:
- Budgets: token/time/cost per project; show meters; deny or degrade at soft caps.
- SLOs: max acceptable interactive latency; scheduler prefers local when remote would miss it.
- Acceptance checks in CI: golden tasks measure local vs offload deltas; fail builds that regress beyond thresholds.
- Kill switches: config flags to turn off clustering, experiments, or deep tracing without code changes.

Net assessment: a small fixed overhead and occasional reduced peak throughput in exchange for safety, explainability, and federation. Dominant costs (inference, browser work, large retrievals) dwarf these overheads if the fast path remains default.
