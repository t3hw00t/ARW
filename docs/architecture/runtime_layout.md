# Runtime Layout
Updated: 2025-09-16
Type: Explanation

Treat latency as a first‑class requirement. Bias the runtime for bursty, interactive work.

Principles
- Interactive gets headroom; background yields.
- Stream earliest useful work first; refine progressively.
- Keep connections warm and avoid re‑dial when possible.

Recommendations (current service aligns with these):
- Separate the interactive control plane (HTTP/API, SSE, read‑models) from background workers. Tokio has no task priorities—use separate runtimes/processes when necessary.
- Yield in long loops (e.g., GC, migration) using `tokio::task::yield_now()` to avoid starving the reactor.
- Use `spawn_blocking` or a dedicated pool for blocking I/O/CPU.
- Assign higher CPU/memory weights to the control plane using systemd (maps to cgroup v2):

```
[Service]
CPUWeight=900
IOWeight=900
MemoryLow=256M
```

Wire/protocol
- SSE + JSON Patch for read‑models; `Last-Event-ID` acks on connect; best‑effort replay via `?replay=N`.
- Budget stream cadence: first event ≤150 ms; steady cadence ≤250 ms.

Implementation layout (current)
- Legacy bridge code has been removed; the unified server now owns `/admin/*`, `/debug`, and static assets directly.
- `bootstrap::run()` wires the bus, resources, read-model fan-out, and HTTP server before spawning background loops.
- Background workers (models GC, snappy, metrics) are launched from a dedicated helper so control-plane tasks stay short-lived.
- The event bus remains the nucleus: HTTP handlers, orchestrator queues, and the kernel journal all publish through the same path, keeping the object graph authoritative.
