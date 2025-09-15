---
title: Egress Schemas
---

# Egress Schemas

Updated: 2025-09-13
Type: Reference

Status: Planned

This page anchors the two minimal JSON Schemas introduced for the policy‑backed egress firewall work:

- Policy: Network Scopes & Leases — spec/schemas/policy_network_scopes.json
- Egress Ledger Entry — spec/schemas/egress_ledger.json

## Policy: Network Scopes & Leases
- Defines project‑level network posture (`off|public|allowlist|custom`).
- `scopes[]`: static allow/deny rules by host/host_glob/ip/cidr + port(s) + protocols.
- `leases[]`: time‑boxed grants that reference a scope or embed one inline; include `issued_by`, `reason`, and expirations.

Preview fields
- `posture`, `scopes[].{action,host,host_glob,ip,cidr,ports,protocols}`, `leases[].{scope_ref|scope_inline,expires_at}`

Use
- Render in UI as “Network Posture” with add/remove scopes and create lease prompts.
- Push to Workers; enforce via per‑node egress gateway + DNS guard.

## Egress Ledger Entry
- Normalized JSONL record per decision: allow/deny with reason codes.
- Attributes include posture, project/episode/node attribution, destination, bytes, duration, optional cost estimate, matched scope/lease ids.

Preview fields
- `decision`, `reason_code`, `dest.{host,port,protocol}`, `bytes_out/in`, `est_cost_usd`, `scope_id`, `lease_id`.

Use
- UI “Egress Ledger” tab with filters (episode, domain, node) and quick revoke/kill.
- Pre‑offload preview can be rendered from the same shape with `decision=preview` (non‑ledger), then committed upon decision.

See also
- Architecture → Egress Firewall
- Guide → Network Posture
- API & Schema → how to fetch and validate schemas
 - Developer → [Egress Ledger Helper (Builder)](../developer/style.md#egress-ledger-helper-builder)
