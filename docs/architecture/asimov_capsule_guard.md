---
title: Asimov Capsule Guard
---

# Asimov Capsule Guard
{ .topic-trio style="--exp:.5; --complex:.7; --complicated:.6" data-exp=".5" data-complex=".7" data-complicated=".6" }

Updated: 2025-10-12
Status: Alpha (hardening in progress)
Type: Explanation

The Asimov Capsule Guard turns the "mind virus" idea into an enforceable feature: a lightweight policy capsule that refreshes itself at every critical runtime boundary, keeping safety rules and leases in effect without piling up irreversible denies.

## Current Behavior & Gaps
- ✅ Capsule adoption runs in the unified server middleware: any request carrying `X-ARW-Capsule` is verified via the RPU, cached, and published to `/state/policy/capsules`. Legacy gate headers now error (410). Lease metadata (`lease_duration_ms`, `renew_within_ms`) travels with the capsule and surfaces in the read model.
- ✅ Capsule fingerprints ignore the signature blob, so re-signed payloads that keep identical policy data do not thrash adoption notifications or read-model patches.【F:apps/arw-server/src/capsule_guard.rs†L430-L438】
- ✅ Legacy failures emit `policy.capsule.failed` and `policy.decision` events so operators can observe rejected attempts.
- ✅ Guardrail preset apply events (`policy.guardrails.applied`) attach `corr_id`/`request_id`, and the HTTP response echoes the same metadata so audits can link preset changes to the initiating request.【F:apps/arw-server/src/api/policy.rs†L210】
- ✅ `gating::adopt_capsule` keeps capsule denies and contracts in a runtime lease layer instead of the immutable hierarchy list, so guardrails expire or renew without a restart.【F:crates/arw-core/src/gating.rs†L248-L353】
- ✅ The Regulatory Provenance Unit returns a lease outcome and the middleware emits `policy.capsule.applied` and `policy.capsule.expired`; the capsules read model now patches only when the underlying state changes (new capsule, hop countdown, or expiry) to avoid noisy updates.【F:crates/arw-core/src/rpu.rs†L205-L220】【F:apps/arw-server/src/capsule_guard.rs†L257-L279】
- ✅ Operators can run `arw-cli capsule status` to fetch `/state/policy/capsules` with renewal windows, accessibility hints, and expiry countdowns for quick audits.
- ✅ Emergency teardown hooks live at `/admin/policy/capsules/teardown` and in `arw-cli capsule teardown`, publishing `policy.capsule.teardown` events for audits before patching the read-model.
- ⚠️ Capsule propagation still needs tighter integration with downstream executors (connectors, logic units) so every tool hop refreshes leases automatically.

The remaining gap is operational coverage: workers and higher-level runners still require passive refresh hooks so a capsule keeps renewing even when no HTTP request crosses the middleware.

## Feature Shape
- **Capsule lifecycle leasing.** Capsules carry reversible denies and contract windows that refresh when the runtime replays them, keeping protections alive without mutating the immutable deny sets.
- **Runtime touchpoints.** Every task ingress (`actions`), tool execution (`tools_exec`), and network boundary (egress proxy, `/egress/preview`) triggers capsule refresh via a cached, verified payload.
- **Operational visibility.** Adoption emits dedicated topics (`policy.capsule.applied`, `policy.capsule.expired`) and exposes read-model snapshots so operators can audit what guardrails are active per project/episode.

## Integration Plan
### Phase 0 — Observability & Data Contracts
1. ✅ Extend `arw-protocol::GatingCapsule` with explicit lease semantics (renewal window, scoped denies) and document them in OpenAPI/MCP specs.
2. ✅ Structured events (`policy.capsule.applied`/`policy.capsule.failed`/`policy.capsule.expired`) fire from the middleware, and `/state/policy/capsules` patches whenever adoption, renewal, hop countdown, or expiry materially changes the state. The read-model now enriches each capsule with derived fields (`status`, `status_label`, `aria_hint`, `expires_in_ms`, `renew_in_ms`, ISO timestamps) so Launcher and CLI surfaces can expose countdowns with accessible narration.
3. ✅ Instrument admin middleware and policy decisions with correlation IDs linking capsule adoption to downstream allows/denies.【F:apps/arw-server/src/capsule_guard.rs†L846】

### Phase 1 — Gating Runtime Rework
1. ✅ Replace immutable hierarchy denies in `arw_core::gating` with a layered view: boot config denies, capsule leases, and user runtime toggles (all with TTL/renewal).
2. ✅ Add a lightweight sweeper inside gating that drops expired capsule leases and refreshes contracts without requiring a restart.
3. ✅ Expose APIs for snapshotting effective policy (config + capsules + leases) so `/state/policy` and the admin UI render consolidated guardrails.

### Phase 2 — Capsule Propagation Hooks
1. ✅ Cache the last verified capsule per session (admin token, project) and replay it automatically before `tools_exec::run`, orchestrator task dispatch, and policy evaluation entry points.
2. ✅ Teach the egress proxy to request a capsule refresh before allowing external connections, ensuring the guard is in place for network operations. Regression tests cover the egress policy pathway.
3. ✅ Update Logic Unit / Recipe runners to request capsule refresh when executing automation bundles so packaged strategies inherit the guard. Installer flow now refreshes capsules and ships with coverage.

### Phase 3 — UX & Operational Controls
1. ✅ (Launcher 0.2.0-dev / Admin console) Capsule presets land in the Launcher and the web models page: the **Strict Egress** toggle ships a signed capsule (`configs/capsules/strict_egress.json`), surfaces renewal windows, and shows TTL countdowns as capsules auto-refresh (2025-10-12).
2. ✅ Provide CLI/admin endpoints to mint, inspect, and revoke capsules—including presets and audit trails—via `/admin/policy/capsules/{presets,adopt,audit,teardown}` and the associated CLI commands (`arw-cli capsule preset list|adopt`, `arw-cli capsule audit`, `arw-cli capsule teardown`).
3. ✅ Documented capsule deployment patterns: boot-time seeding via `configs/gating.toml` / env, Launcher/CLI runtime layers for incidents, and trust-store rotation via `/admin/rpu/reload` (2025-10-12).

## Deployment Patterns

### Seed baseline guardrails
- Store signed capsule manifests under `configs/capsules/`; the repository now ships `configs/capsules/strict_egress.json` as the canonical preset.
- Reference baseline denies via `configs/gating.toml` or the `ARW_GATING_DENY` environment variable so guardrails load before any runtime traffic. Capsules adopted during boot remain immutable until you rotate the configuration.
- When packaging for teams, include the preset capsule and update `configs/trust_capsules.json` with the public key that will sign rotations.

### Layer runtime presets and incidents
- Use the Launcher (Models → Egress → **Policy capsules**) or the admin models page to toggle posture presets. The card surfaces renewal/expiry countdowns so operators can watch leases in real time.
- CLI automation: `arw-cli capsule preset list --base http://127.0.0.1:8091` enumerates available presets and `arw-cli capsule preset adopt --id capsule.strict-egress --base http://127.0.0.1:8091` applies one through the new `/admin/policy/capsules/{presets,adopt}` endpoints. Legacy flows that adopt a local file remain available via `arw-cli capsule adopt configs/capsules/strict_egress.json`.
- Tail capsule events with `arw-cli capsule audit --base http://127.0.0.1:8091 --limit 25` or use the admin models page audit panel for a live view.
- Raw HTTP flows can replay the header directly when needed:

  ```bash
  HDR=$(jq -c '.' configs/capsules/strict_egress.json)
  curl -s -H "X-ARW-Capsule: $HDR" -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" \
    http://127.0.0.1:8091/state/policy/capsules >/dev/null
  ```

- Inspect active capsules and renewal health with `arw-cli capsule status --base http://127.0.0.1:8091` or the UI; remove layers with `arw-cli capsule teardown --id capsule.strict-egress` / `--all` or the on-page teardown controls.

### Rotate trust issuers
- Trust issuers live in `configs/trust_capsules.json`. Use `arw-cli capsule trust list` to inspect the current set and `arw-cli capsule trust rotate --id local-admin --reload` to generate a new ed25519 pair, persist it, and trigger `/admin/rpu/reload`.
- Reload the trust store without restarting the service via `curl -X POST -H "X-ARW-Admin: $ARW_ADMIN_TOKEN" http://127.0.0.1:8091/admin/rpu/reload`.
- Use `/admin/rpu/trust` or `arw-cli capsule status --json` to audit the issuer list after rotation.

## Dependencies & Interactions
- Builds on `policy_leases` for capability prompts and telemetry, and `guardrail_gateway` for egress enforcement.
- Requires the RPU trust store to remain authoritative for issuers (`ARW_TRUST_CAPSULES`) and uses existing gating environment overrides (`ARW_GATING_DENY`) for bootstrap policy.
- Publishes telemetry over the shared event bus so Debug UI lenses (Events, Policy sidecar) can visualize adoption without bespoke wiring.

Delivering this plan converts the aspirational "mind virus" into a measurable, renewable guardrail that the runtime can enforce at every step without sacrificing reversibility or operator clarity. Outstanding ⏭ items are tracked in the Security & Admin backlog so they can mature without blocking day-to-day stability.
