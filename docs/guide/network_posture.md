---
title: Network Posture
---

# Network Posture

Updated: 2025-09-12

Status: Planned
Type: How‑to

Network posture is a per‑project setting that translates policy into enforceable egress controls via a host‑local gateway and DNS guard. It remains opt‑in and aims to keep the local fast‑path fast.

## Modes (Per Project)
- Off: no gateway enforcement (dev only).
- Public only: allow common public domains (package registries, docs, model hubs); block private/risky destinations.
- Allowlist: only explicitly permitted domains/ports are allowed.
- Custom: start from allowlist and add scoped exceptions with TTL leases.

## Leases & Prompts
- Prompts request a lease (duration + scope) when a tool needs broader access.
- Decisions are logged and visible in the sidecar; deny wins.

## Preview & Ledger
- Pre‑offload dialog shows destination, payload summary, and estimated cost.
- Egress ledger records decisions, bytes, and attribution (episode/project/node).

## Planned Configuration (Preview)
- `ARW_NET_POSTURE`: `off|public|allowlist|custom`
- `ARW_EGRESS_PROXY_ENABLE`: `1` (per node)
- `ARW_DNS_GUARD_ENABLE`: `1` (per node)

See also: Architecture → Egress Firewall; Policy; Security Hardening; Clustering.
