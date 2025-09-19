---
title: Human‑in‑the‑Loop
---

# Human‑in‑the‑Loop

Updated: 2025-09-18
Type: How‑to

Status: **Planned.** The legacy `arw-svc` carried a staging queue, but the unified `arw-server` has not yet reintroduced it. The launcher sidecar shows only static copy.

This page tracks the migration plan for Human-in-the-loop approvals on the new stack.

## Implementation Plan (`arw-server` + UI)

1. **Staging queue** — persist pending actions in the kernel with explicit leases and expirations, and expose a `/state/staging/actions` read-model (`t-250918120301-hitl01`).
2. **Escalations** — publish `policy.decision` events whenever an action requires review so subscribers (sidecar, CLI) can badge attention.
3. **Sidecar approvals** — wire the existing sidecar panel to list staged actions with evidence previews and approve/deny calls (`t-250918120305-hitl02`).
4. **Modes** — implement per-project modes (auto, ask-once, always-review) as policy hints so admins can tune risk appetite per workspace.

## Configure & Observe

- Set `ARW_ACTION_STAGING_MODE` to `auto` (default), `ask`, or `always`. Pair with `ARW_ACTION_STAGING_ALLOW` (CSV list of action kinds) to whitelist low-risk calls when running in ask mode.
- Use `ARW_ACTION_STAGING_ACTOR` to label who is submitting staged requests; the label is stored with the queue entry.
- Review the queue via `GET /state/staging/actions` or the `state.read.model.patch` stream (id `staging_actions`). Approve or deny entries with `POST /staging/actions/{id}/approve|deny`.

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
