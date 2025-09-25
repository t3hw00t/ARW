---
title: Asimov Capsule Guard
---

# Asimov Capsule Guard
{ .topic-trio style="--exp:.5; --complex:.7; --complicated:.6" data-exp=".5" data-complex=".7" data-complicated=".6" }

Updated: 2025-09-26
Status: Alpha (hardening in progress)
Type: Explanation

The Asimov Capsule Guard turns the "mind virus" idea into an enforceable feature: a lightweight policy capsule that refreshes itself at every critical runtime boundary, keeping safety rules and leases in effect without piling up irreversible denies.

## Current Behavior & Gaps
- ✅ Capsule adoption runs in the unified server middleware: any request carrying `X-ARW-Capsule` is verified via the RPU, cached, and published to `/state/policy/capsules`. Legacy `X-ARW-Gate` headers now error (410). Lease metadata (`lease_duration_ms`, `renew_within_ms`) travels with the capsule and surfaces in the read model.
- ✅ Capsule fingerprints ignore the signature blob, so re-signed payloads that keep identical policy data do not thrash adoption notifications or read-model patches.【F:apps/arw-server/src/capsule_guard.rs†L430-L438】
- ✅ Legacy failures emit `policy.capsule.failed` and `policy.decision` events so operators can observe rejected attempts.
- ✅ `gating::adopt_capsule` keeps capsule denies and contracts in a runtime lease layer instead of the immutable hierarchy list, so guardrails expire or renew without a restart.【F:crates/arw-core/src/gating.rs†L248-L353】
- ✅ The Regulatory Provenance Unit returns a lease outcome and the middleware emits `policy.capsule.applied` and `policy.capsule.expired`; the capsules read model now patches only when the underlying state changes (new capsule, hop countdown, or expiry) to avoid noisy updates.【F:crates/arw-core/src/rpu.rs†L205-L220】【F:apps/arw-server/src/capsule_guard.rs†L257-L279】
- ⚠️ Capsule propagation still needs tighter integration with downstream executors (connectors, logic units) so every tool hop refreshes leases automatically.

The remaining gap is operational coverage: workers and higher-level runners still require passive refresh hooks so a capsule keeps renewing even when no HTTP request crosses the middleware.

## Feature Shape
- **Capsule lifecycle leasing.** Capsules carry reversible denies and contract windows that refresh when the runtime replays them, keeping protections alive without mutating the immutable deny sets.
- **Runtime touchpoints.** Every task ingress (`actions`), tool execution (`tools_exec`), and network boundary (egress proxy, `/egress/preview`) triggers capsule refresh via a cached, verified payload.
- **Operational visibility.** Adoption emits dedicated topics (`policy.capsule.applied`, `policy.capsule.expired`) and exposes read-model snapshots so operators can audit what guardrails are active per project/episode.

## Integration Plan
### Phase 0 — Observability & Data Contracts
1. ✅ Extend `arw-protocol::GatingCapsule` with explicit lease semantics (renewal window, scoped denies) and document them in OpenAPI/MCP specs.
2. ✅ Structured events (`policy.capsule.applied`/`policy.capsule.failed`/`policy.capsule.expired`) fire from the middleware, and `/state/policy/capsules` patches whenever adoption, renewal, hop countdown, or expiry materially changes the state.
3. ⏭ (Backlog) Instrument admin middleware and policy decisions with correlation IDs linking capsule adoption to downstream allows/denies.

### Phase 1 — Gating Runtime Rework
1. ✅ Replace immutable hierarchy denies in `arw_core::gating` with a layered view: boot config denies, capsule leases, and user runtime toggles (all with TTL/renewal).
2. ✅ Add a lightweight sweeper inside gating that drops expired capsule leases and refreshes contracts without requiring a restart.
3. ✅ Expose APIs for snapshotting effective policy (config + capsules + leases) so `/state/policy` and the admin UI render consolidated guardrails.

### Phase 2 — Capsule Propagation Hooks
1. ✅ Cache the last verified capsule per session (admin token, project) and replay it automatically before `tools_exec::run`, orchestrator task dispatch, and policy evaluation entry points.
2. ⏭ (Backlog) Teach the egress proxy to request a capsule refresh before allowing external connections, ensuring the guard is in place for network operations.
3. ⏭ (Backlog) Update Logic Unit / Recipe runners to request capsule refresh when executing automation bundles so packaged strategies inherit the guard.

### Phase 3 — UX & Operational Controls
1. ⏭ (Backlog) Add UI toggles to treat capsules as posture presets (e.g., "Strict Egress"), showing TTL countdowns and renewal status.
2. ⏭ (Backlog) Provide CLI/admin endpoints to mint, inspect, and revoke capsules—including emergency teardown hooks for misconfigurations.
3. ⏭ (Backlog) Document deployment patterns: seeding base capsules via `configs/gating.toml` or env vars, layering runtime capsules for incidents, and rolling keys via the trust store.

## Dependencies & Interactions
- Builds on `policy_leases` for capability prompts and telemetry, and `guardrail_gateway` for egress enforcement.
- Requires the RPU trust store to remain authoritative for issuers (`ARW_TRUST_CAPSULES`) and uses existing gating environment overrides (`ARW_GATING_DENY`) for bootstrap policy.
- Publishes telemetry over the shared event bus so Debug UI lenses (Events, Policy sidecar) can visualize adoption without bespoke wiring.

Delivering this plan converts the aspirational "mind virus" into a measurable, renewable guardrail that the runtime can enforce at every step without sacrificing reversibility or operator clarity. Outstanding ⏭ items are tracked in the Security & Admin backlog so they can mature without blocking day-to-day stability.
