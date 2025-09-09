ARW Clustering & Connectors (MVP -> Medium Depth)
Updated: 2025-09-09

- Default behavior remains single-process using a local in-memory queue and event bus.
- Medium-depth scale-out uses a pluggable Queue and Bus abstraction with NATS JetStream as the recommended backend.

Why NATS JetStream
- Simple operational model, low latency, at-least-once with durable consumer groups.
- Built-in discovery and clustering; easy to add nodes (“connect a second”).
- Good bridgeability to edge environments and compatible with future p2p overlays.
- Can co-exist with Bitcoin ecosystem tooling; we can bridge ZMQ publishers from `bitcoind` into NATS subjects for event-driven workflows.

Bitcoin/Blockchain alignment
- Identity: leverage secp256k1 keys (Lightning/Bitcoin-style) for mTLS/Noise handshakes between cores and connectors.
- Event ingress: optional ZeroMQ adapter to consume `bitcoind` notifications, republish into Bus/Queue.
- Provenance: sign connector binaries/plugins with Sigstore; attestments can be anchored or mirrored alongside Bitcoin timestamping services if desired.

MVP in this repo
- `arw-core::orchestrator`: Task model, LocalQueue with leases, and Orchestrator façade.
- `arw-events`: EventBus trait with LocalBus; façade `Bus` keeps existing SSE endpoints working.
- `arw-protocol`: `ConnectorHello`, `ConnectorHeartbeat` types.
- `arw-svc`: new `/tasks/enqueue` (debug/admin-gated) and a local background worker to execute minimal built-in tools via the queue.

Config (configs/default.toml)
```
[cluster]
enabled = false
bus = "local"   # or "nats" (feature: arw-core/nats)
queue = "local" # or "nats" (feature: arw-core/nats)
# nats_url = "nats://127.0.0.1:4222"
```

Next steps
- Implement `NatsQueue` (JetStream streams, durable consumer groups, ack/nack/lease semantics).
- Add `NatsBus` relay to feed local SSE subscribers.
- Define connector control-plane (gRPC/QUIC) for Hello/Heartbeat/Assignment.
- Optional ZMQ bridge for Bitcoin Core notifications -> Bus/Queue.

