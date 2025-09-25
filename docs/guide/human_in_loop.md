---
title: Human‑in‑the‑Loop
---

# Human‑in‑the‑Loop

Updated: 2025-09-20
Type: How‑to

Status: **Available (phase one).** `arw-server` now stages actions when `ARW_ACTION_STAGING_MODE` demands review, backs approvals with the kernel, and exposes live views in `/state/staging/actions` and the Debug UI. Launcher sidecar cards render the same data; richer evidence previews remain on the roadmap.

This page explains the shipped experience and the remaining roadmap for Human-in-the-loop approvals on the unified stack.

## What’s Live Today

- Kernel persistence: staged actions, approvals, and denials are recorded in SQLite with timestamps, reviewers, and linkage back to the action id.
- Unified API surface:
  - `POST /actions` responds with `{ staged: true }` when submissions enter the queue.
  - `GET /state/staging/actions` enumerates pending and decided entries (`status`, `project`, `requested_by`, `created`).
  - `POST /staging/actions/{id}/approve|deny` promotes or rejects staged items, emits `staging.decided`, and replays `actions.submitted` for downstream consumers.
- Live surfaces: `/admin/debug` lists staged items with approve/deny controls; the launcher mirrors the feed and badges attention alongside notifications.
- Event telemetry: `staging.pending` and `staging.decided` fire through `/events`; policy denials continue to emit `policy.decision` when leases are missing.

## Next Enhancements

1. **Evidence & context lanes** — inline previews, diffs, and artifact links in the sidecar (`t-250918120305-hitl02`).
2. **Per-project policy hints** — richer `always/ask/auto` presets rooted in project posture, plus escalation rules.
3. **Queue ergonomics** — dedupe on action cache hashes, paging for long queues, and SLA alerts to keep approvals timely.
4. **Audit trails** — fold reviewer notes into the contribution ledger view with filters and retention controls.

## Configure & Observe

- `ARW_ACTION_STAGING_MODE`: `auto` (default, queue only when policies demand), `ask` (queue everything except an allowlist), or `always`.
- `ARW_ACTION_STAGING_ALLOW`: comma-delimited action kinds that bypass staging when you run in ask mode.
- `ARW_ACTION_STAGING_ACTOR`: optional label recorded with each staged entry (helpful for multi-seat installs).
- `GET /state/staging/actions`: returns pending or decided entries; add `?status=pending` and `?limit=500` for focused dashboards.
- `state.read.model.patch` with id `staging_actions`: feeds sidecars and headless clients with incremental updates.
- `POST /staging/actions/{id}/approve|deny`: send `{ "decided_by": "name" }` or `{ "reason": "why" }` (JSON body) to approve or deny a queued action.

Example:

```bash
curl -s -X POST http://127.0.0.1:8091/staging/actions/$ID/approve \
  -H 'content-type: application/json' \
  -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
  -d '{"decided_by":"reviewer.cc"}' | jq
```

## Evidence & Audit

- Require provenance links for every staged action; approvals should capture the reviewer, timestamp, and diff of the requested change.
- Store final decisions in the contribution ledger so audits can reconstruct who approved what.

## Optimization Tips

- Reuse the Action Cache hash as the dedupe key so repeated requests for the same change do not clog the queue.
- Bound queue size per project; spill older entries to disk with notifications instead of blocking new work indefinitely.
- Keep staging decisions idempotent — approving twice should be a no-op — by routing through `/actions/:id/state` transitions.

## Related Work

- Backlog: `t-250918120301-hitl01`, `t-250918120305-hitl02`
- Roadmap: Pack Collaboration → Human-in-the-loop staging
- See also: [guide/policy_permissions.md](policy_permissions.md), [architecture/capability_consent_ledger.md](../architecture/capability_consent_ledger.md)
