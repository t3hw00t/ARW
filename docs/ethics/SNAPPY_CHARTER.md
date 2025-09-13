# Snappy by Default — Product Charter

Primary metric: interaction‑to‑first‑feedback (I2F) ≤ 50 ms (p95); first useful partial ≤ 150 ms; steady stream cadence ≤ 250 ms/chunk; full result ≤ 2 s where feasible.

- We always show something within 50 ms (echo, ack, progress).
- Under load, we degrade fidelity before latency (partial results, progressive refinement).
- Interactive work has reserved capacity; background work yields.
- Streaming by default; snapshots only when explicitly requested.
- Every PR must keep or improve these budgets (see CI gates below).

## A. OS & Process Level (Cachy‑style “burst first”)

- Separate the interactive control plane (HTTP/API, event mux, SSE) from worker processes using cgroup v2; give the control plane higher CPU weight and memory protection so it stays responsive during bursts. Example: `CPUWeight=900` for control‑plane service, `CPUWeight=200` for workers; use `MemoryLow=` to protect the interactive set. (systemd maps to cgroup v2’s `cpu.weight` / `memory.low`).
- Keep long‑lived connections (HTTP/2 or SSE) from UI to server; avoid reconnection tax. Prefer SSE for event stream.
- Ship tuned TCP/IO/network defaults in containers/units, scoped to services (not global sysctls).

### systemd unit override (interactive control plane)

```
# /etc/systemd/system/agent-hub.service.d/snappy.conf
[Service]
CPUWeight=900
IOWeight=900
MemoryLow=256M
# keep connections and sockets warm
Restart=always
```

## B. Runtime Scheduling (Rust/Tokio)

- Emulate BORE’s “burst‑oriented” behavior by classifying tasks: interactive (user‑initiated), service (state/read‑model), background (indexing/training).
- Run two runtimes or processes when practical: one dedicated runtime for interactive work; another for background. Tokio has no task priorities—separate runtimes/processes is the recommended pattern.
- Use explicit yielding (`tokio::task::yield_now()`) in long async loops and move blocking I/O/CPU into `spawn_blocking` or a separate worker pool.

## C. Wire/Protocol (make it feel instantaneous)

- Stream deltas, not snapshots: push JSON Patch (RFC 6902) over SSE; clients rehydrate state locally and apply patches. Resume with `Last-Event-ID` where possible.
- Budget the stream: first event ≤150 ms with minimal payload (“ack + plan”), then steady 100–250 ms cadence.

Contract excerpt (server):

- Transport: `text/event-stream` (SSE), HTTP/2 preferred
- Event types: `Service.Connected` (ack/resume), `State.*.Patch` (JSON Patch), `*.Notice`, `*.Done`
- Patch format: `application/json-patch+json` (RFC 6902)
- Resume: `Last-Event-ID` header is accepted; `?replay=N` also supported
- Budgets: first event ≤150 ms; cadence ≤250 ms

## D. Data & Cache Path (zero‑wait hot paths)

- Pre‑warm: load hot schemas, recipe manifests, and most‑used tools’ descriptors at boot; keep a tiny in‑memory index for zero‑allocation lookup.
- Action Cache for tool calls (content‑addressed; stable hashing via JSON Canonicalization) so repeated user flows are instant; background refresh near TTL expiry.
- Exact + semantic caches for model/tool responses; return cached partials within 150 ms and continue streaming refinement when needed.
- Negative caches (no‑answer/tool‑error) for a short TTL to avoid thrash.

## E. UI Behavior (perceived speed)

- Immediate echo of input and “live plan” chips; optimistic UI for safe operations.
- Never block on “perfect” context assembly—show partials and refine.
- Prefer skeletons over spinners; keep frame budget ≤16 ms for animations.

## F. Observability for “feel”

- Export RED metrics for the interactive surface (rate, errors, duration with p95/p99), broken down by step: accept, plan, first‑patch, final. Gate merges on p95 budgets.
- Resource probes via the USE method to catch micro‑saturation (bursts, not 1‑min averages).

## G. CI / CD Gates (non‑negotiable)

- Latency tests: benchmark `i2f`, `first_partial`, and `steady_cadence` with a synthetic workload; fail PRs that regress p95 beyond budget.
- Cold‑start test: boot service, measure time to first event; require ≤500 ms for the control plane.
- Allocation budget: forbid >N allocations in hot handlers (rough guard for polish).
- Docs gate: PR must state impact on these budgets (kept/improved/regressed).

## H. Default Configuration (repo‑kept)

```
snappy:
  budgets:
    i2f_p95_ms: 50
    first_partial_p95_ms: 150
    cadence_ms: 250
    full_result_p95_ms: 2000
  gating:
    fail_on_regressions: true
    protected_endpoints: ["/debug", "/state/*", "/chat/*"]

runtimes:
  control_plane:
    workers: N_cpu/2            # API + SSE + read-models
  background:
    workers: N_cpu/2            # tool calls, indexing
policies:
  long_loops_must_yield: true   # use tokio::task::yield_now()
```

## I. Roll‑out Order

1) Turn on SSE + JSON Patch deltas and budgets in the server and UI.
2) Split control plane from workers; add cgroup weights via systemd overrides.
3) Pre‑warm hot descriptors/schemas; enable action cache + stable hashing.
4) Add latency tests + RED dashboards; set p95 gates.
5) Iterate: watch p95s, shave cold starts, reduce bytes on the wire.

## J. Why “CachyOS‑style” Works Here

CachyOS’s BORE is about resilient responsiveness under varied load; we mirror the effect by classifying and reserving capacity for bursts, streaming earliest useful work first, and biasing every decision toward perceived latency. The sysctl/scheduler mindset becomes: “interactive gets headroom; everything else yields.”

