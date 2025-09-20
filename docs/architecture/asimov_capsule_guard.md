---
title: Asimov Capsule Guard
---

# Asimov Capsule Guard
{ .topic-trio style="--exp:.5; --complex:.7; --complicated:.6" data-exp=".5" data-complex=".7" data-complicated=".6" }

Updated: 2025-09-20
Type: Plan

The Asimov Capsule Guard turns the "mind virus" idea into an enforceable feature: a lightweight policy capsule that refreshes itself at every critical runtime boundary, keeping safety rules and leases in effect without piling up irreversible denies.

## Current Behavior & Gaps
- Capsule adoption now runs in the unified server’s capsule middleware: any request carrying `X-ARW-Capsule` (or the legacy `X-ARW-Gate`) is verified via the RPU, cached, and published to `/state/policy/capsules`. The guard still needs auto-replay hooks for tools/orchestrator and richer TTL semantics.
- `gating::adopt_capsule` copies every deny into the immutable hierarchy set and expands contracts into the shared contract list, making each capsule permanent until the process restarts; only test helpers can reset state.【F:crates/arw-core/src/gating.rs†L309-L353】
- The Regulatory Provenance Unit (RPU) skeleton verifies a capsule and pushes it into gating but does not emit telemetry beyond existing `policy.decision` events, leaving adoption opaque to the UI and ledger.【F:crates/arw-core/src/rpu.rs†L200-L231】

These gaps prevent capsules from acting like an "always-enforced" guardrail and make experimentation risky—misconfigured denies stick forever.

## Feature Shape
- **Capsule lifecycle leasing.** Capsules carry reversible denies and contract windows that refresh when the runtime replays them, keeping protections alive without mutating the immutable deny sets.
- **Runtime touchpoints.** Every task ingress (`actions`), tool execution (`tools_exec`), and network boundary (egress proxy, `/egress/preview`) triggers capsule refresh via a cached, verified payload.
- **Operational visibility.** Adoption emits dedicated topics (`policy.capsule.applied`, `policy.capsule.expired`) and exposes read-model snapshots so operators can audit what guardrails are active per project/episode.

## Integration Plan
### Phase 0 — Observability & Data Contracts
1. Extend `arw-protocol::GatingCapsule` with explicit lease semantics (renewal window, scoped denies) and document them in OpenAPI/MCP specs.
2. Structured events (`policy.capsule.applied`/`policy.capsule.failed`) now fire from the capsule middleware, and `/state/policy/capsules` exposes the cached view; RPU telemetry still needs adoption for runtime refresh and expiring leases.
3. Instrument admin middleware and existing policy decisions with correlation IDs linking capsule adoption to downstream allows/denies.

### Phase 1 — Gating Runtime Rework
1. Replace immutable hierarchy denies in `arw_core::gating` with a layered view: boot config denies, capsule leases, and user runtime toggles (all with TTL/renewal).
2. Add a scheduler inside gating that sweeps expired capsule leases and downgrades contracts safely without requiring a restart.
3. Expose APIs for snapshotting effective policy (config + capsules + leases) so `/state/policy` can render consolidated guardrails.

### Phase 2 — Capsule Propagation Hooks
1. Cache the last verified capsule per session (admin token, project) and replay it automatically before `tools_exec::run`, orchestrator task dispatch, and policy evaluation entry points.
2. Teach the egress proxy to request a capsule refresh before allowing external connections, ensuring the guard is in place for network operations.
3. Update Logic Unit / Recipe runners to request capsule refresh when executing automation bundles so packaged strategies inherit the guard.

### Phase 3 — UX & Operational Controls
1. Add UI toggles to treat capsules as posture presets (e.g., "Strict Egress"), showing TTL countdowns and renewal status.
2. Provide CLI/admin endpoints to mint, inspect, and revoke capsules—including emergency teardown hooks for misconfigurations.
3. Document deployment patterns: seeding base capsules via `configs/gating.toml` or env vars, layering runtime capsules for incidents, and rolling keys via the trust store.

## Dependencies & Interactions
- Builds on `policy_leases` for capability prompts and telemetry, and `guardrail_gateway` for egress enforcement.
- Requires the RPU trust store to remain authoritative for issuers (`ARW_TRUST_CAPSULES`) and uses existing gating environment overrides (`ARW_GATING_DENY`) for bootstrap policy.
- Publishes telemetry over the shared event bus so Debug UI lenses (Events, Policy sidecar) can visualize adoption without bespoke wiring.

Delivering this plan converts the aspirational "mind virus" into a measurable, renewable guardrail that the runtime can enforce at every step without sacrificing reversibility or operator clarity.
