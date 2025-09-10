Hierarchy Orchestrator (Design + MVP)
Updated: 2025-09-10

Goals
- Allow cores to negotiate roles and relationships dynamically (root/regional/edge/connector/observer).
- Keep the hot data plane decentralized (queue/bus) while the control plane is light.
- Preserve single-node defaults; enable hierarchy via opt-in config.

Model
- Roles: Root, Regional, Edge, Connector, Observer.
- Topology: a DAG where edges represent delegation/parenting (non-exclusive by default).
- Epoch: monotonic counter incremented on local topology changes to resolve conflicts.

Negotiation (protocol types in arw-protocol)
- CoreHello(id, role, capabilities, scope_tags, epoch, nonce)
- CoreOffer(from_id, proposed_role, parent_hint, shard_ranges, capacity_hint)
- CoreAccept(child_id, parent_id, role, epoch)

Gating & Policy Capsules
- All interactions can optionally carry a `GatingCapsule` via HTTP header `X-ARW-Gate`.
- Capsules propagate immutable denies and deny contracts (role/node/tags/time windows; auto-renew) — ephemeral and renegotiated on restart.
- Ingress/Egress Guards enforce policy at the beginning and end of action/memory stacks (e.g., `io:ingress:task.kind`, `io:egress:task.kind`).

Regulatory Provenance Unit (planned)
- Signature verification for capsules, trust store, ABAC policy (Cedar) for adoption, hop TTL and propagation scope, and an ephemeral adoption ledger.

MVP in this repo
- Local `arw-core::hierarchy` state, with APIs to get/set role and minimal parent/child links.
- HTTP endpoints in `arw-svc`:
  - GET /hierarchy/state — returns local state, emits Hierarchy.State
  - POST /hierarchy/role { role } — sets role, emits Hierarchy.RoleChanged
- AsyncAPI channels added: Hierarchy.RoleChanged, Hierarchy.State.

Next steps
- Control-plane transport: gRPC/QUIC for Hello/Offer/Accept with mTLS.
- NATS-based discovery: subjects like arw.core.hello, arw.core.offer.<id>, arw.core.accept.<id> (JetStream for durability).
- Leader/lease: optional OpenRaft/etcd only for rare global operations; keep hot path queue-driven.
- Policy: OPA/Cedar rules gate role changes, parent selection, shard assignments.

Bitcoin alignment
- Identity: use secp256k1-derived identities for mutual auth (SPIFFE/SPIRE or Noise).
- Event ingress: ZeroMQ bridge from bitcoind -> NATS subjects; region roots filter/aggregate.
