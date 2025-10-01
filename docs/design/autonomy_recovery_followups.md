---
title: Autonomy Recovery Follow-ups
---

# Autonomy Recovery Follow-ups

Updated: 2025-09-27
Type: Implementation record
Status: Completed

## Motivation
Operators still depend on a mostly manual rollback flow after the restructure. The quick script and playbook call out missing endpoints for project snapshots, guardrail presets, and runtime restores; lane budgets also need a first-class management surface. Restoring a minimal set of APIs will shorten recovery loops, improve auditability, and align the docs with the unified server capabilities.

## Scope
- Provide API coverage for capturing/restoring project snapshots during an autonomy rollback.
- Reintroduce guardrail preset management without reopening the legacy gating facade.
- Add an automation hook for restarting managed runtimes that aligns with the new supervisor.
- Expose lane budget updates through the config plane so operations stay declarative.

## Proposed Workstreams
1. **Project Snapshots API** _(shipped)_
   - Endpoint: `POST /projects/{proj}/snapshot` → returns `{ id, path, created_at }`.
   - Storage: reuse `state/projects` dir, placing snapshots under `state/projects/.snapshots/{proj}/{id}` with CAS metadata.
   - Kernel hook: persist snapshot metadata in the existing config snapshot table (or extend kernel with `project_snapshots` if separation is clearer).
   - Permissions: admin token required; emit `projects.snapshot.created` event for audit.
   - Rollback helper: extend `scripts/autonomy_rollback.sh` to invoke the endpoint before manual restore instructions.

2. **Guardrail Preset Apply** _(shipped)_
   - Endpoint: `POST /policy/guardrails/apply` `{ preset: string }`.
   - Implementation uses the existing gating loader (`configs/gating.toml`) and policy reload pipeline; avoid reintroducing `/admin/gating`.
   - Response includes `{ ok: true, preset, policy_version }`; publish `policy.guardrails.applied` event.
   - CLI/script: update rollback helper to call the endpoint; docs call out the new route.

3. **Runtime Restore Hook** _(shipped)_
   - Endpoint: `POST /orchestrator/runtimes/{id}/restore`.
   - Delegate to the managed runtime supervisor (already exposes state via `/state/runtime_supervisor`).
   - Body allows `{ restart: bool, preset?: string }` so future enhancements can warm caches or apply profiles.
   - Emit `runtime.restore.requested`/`runtime.restore.completed` events to track execution and fallback to manual if supervisor offline.

4. **Lane Budget Management** _(shipped)_
   - Define `configs/autonomy/lanes/{lane}.json` (existing manifest) as source of truth.
   - Endpoint: `POST /admin/autonomy/{lane}/budgets` with payload `{ wall_clock, tokens, spend }` (all optional) to patch manifest via Config Plane helper.
   - Internally reuse the config engine; the endpoint just validates and forwards to `config::patch_apply` with `dry_run` support.
   - Update `/state/autonomy/lanes` to include `last_budget_update` and derived headroom metrics.

## Milestones
- **M1:** Snapshot API + docs/playbook/script integration.
- **M2:** Guardrail preset apply endpoint, events, and helper wiring.
- **M3:** Runtime restore hook anchored in supervisor; smoke test coverage.
- **M4:** Lane budget endpoint + `/state/autonomy/lanes` enhancements.

## Open Questions
1. Do we persist project snapshots in CAS or reuse existing compressed archives from the launcher? (Impacts restore latency.)
2. Should guardrail presets be versioned per lane, or is a global preset sufficient?
3. How do we authenticate runtime restore calls in multi-operator environments? (Possibly require capsule leases.)
4. Can lane budget updates piggyback on the config patch audit trail, or do we emit dedicated autonomy events?

## Validation
- Extend integration tests to cover the new endpoints with admin auth and ensure events arrive on the bus.
- Update `just autonomy-rollback` smoke to execute the new APIs end-to-end (with `--dry-run` verifying preview paths).
- Add documentation updates in `docs/ops/trials/autonomy_rollback_playbook.md` once endpoints are implemented.
- `cargo test --package arw-server` exercises the new API surfaces and regression coverage.
