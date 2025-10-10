---
title: Network Posture
---

# Network Posture

Updated: 2025-10-10

Status: Partial (gateway + posture enforcement implemented)
Type: How‑to

Network posture is a per‑project setting that translates policy into enforceable egress controls via a host‑local gateway and DNS guard. It remains opt‑in and aims to keep the local fast‑path fast.

## Modes (Per Project)
- Off / Relaxed: no hostname enforcement (dev only).
- Public / Standard: allow a curated set of public registries (GitHub, crates.io, PyPI, npm, Hugging Face, container registries) plus any host explicitly listed in settings.
- Allowlist / Custom / Strict: only hosts listed in settings are permitted; non-standard ports require explicit entries.

The effective posture is resolved from `POST /egress/settings` or the corresponding `ARW_NET_POSTURE` env var. Hosts are matched case-insensitively with wildcard support (`*.example.com`).

## Lease Overrides
- When posture blocks a host or port, the gateway checks `net:*` leases before denying the egress. Granting a lease such as `net:host:internal.example.com` or `net:port:8443` temporarily widens scope.
- Capsule leases (Asimov Capsule Guard · alpha) refresh these capabilities automatically once adopted.

## Leases & Prompts
- Prompts request a lease (duration + scope) when a tool needs broader access.
- Decisions are logged and visible in the sidecar; deny wins.

## Preview & Ledger
- Pre‑offload dialog shows destination, payload summary, and estimated cost.
- Egress ledger records decisions, bytes, and attribution (episode/project/node).

## Configuration
- `ARW_NET_POSTURE`: `off|public|allowlist|custom`
- `ARW_EGRESS_PROXY_ENABLE`: `1` (per node; preview forward proxy, defaults to enabled)
- `ARW_DNS_GUARD_ENABLE`: `1` (per node, defaults to enabled)
- `ARW_EGRESS_BLOCK_IP_LITERALS`: `1` (block IP-literal hosts for built-in `http.fetch`)
- `ARW_EGRESS_LEDGER_ENABLE`: `1` (log decisions)
- `ARW_EGRESS_MULTI_LABEL_SUFFIXES`: comma-separated extra multi-label suffixes to treat as registrable domains when resolving allowlists/capabilities (e.g., `internal.test,gov.bc.ca`).

Config-driven override:

```toml
[egress]
multi_label_suffixes = ["internal.test", "gov.bc.ca"]
```

Entries should represent effective TLDs (for example `gov.bc.ca`) so that hostnames collapse to the owner label plus the configured suffix.

Preview endpoint
- `POST /egress/preview` dry-runs posture, allowlist, leases, and policy evaluation for a URL/method before running tools.

## Launcher guidance
- When you pivot the desktop launcher to a remote base over plain HTTP, the Home view and satellite windows surface an inline warning with a link to this guide.
- Switch to HTTPS (or tunnel the port) before inviting collaborators or exposing admin surfaces beyond localhost.

See also: Architecture → Egress Firewall; Policy; Security Hardening; Clustering.
