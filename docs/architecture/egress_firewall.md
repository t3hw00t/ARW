---
title: Egress Firewall
---

# Policy‑Backed Egress Firewall
{ .topic-trio style="--exp:.5; --complex:.6; --complicated:.7" data-exp=".5" data-complex=".6" data-complicated=".7" }

Updated: 2025-09-12

This plan adds a lightweight, enforceable egress gateway backed by project policy, with DNS guardrails, filesystem scoping, and leased sensor access. It keeps the fast local path fast and is maintainable by one person per cluster.

## Core Stance
- Soft gates (prompts, tool policies) are necessary but insufficient — prompt‑injection and tool misuse happen.
- Add hard gates: a minimal, host‑local egress firewall every tool/agent must traverse; scope filesystem and sensors. Opt‑in and per‑project.

## Policy → Enforcement
- Extend Policy/Permission model with explicit network scopes: domains, IP/CIDR, ports, protocols, plus TTL leases.
- Make scopes enforceable (not advisory). Deny wins; leases expire automatically.

## Egress Gateway (Per Node)
- One tiny forward proxy on 127.0.0.1 supporting HTTP(S)/WebSocket CONNECT and optionally SOCKS5.
- Enforces allow/deny by domain (SNI/Host) and port without TLS MITM.
- Logs decisions to an egress ledger with project/episode IDs, bytes, and estimated cost.
- Presents a pre‑offload preview in the UI: what leaves, where, and cost.

## Routing (Make It Unskippable)
- Containers/browsers: set proxy env vars and block direct egress in that net namespace; only 127.0.0.1:proxy reachable.
- Host processes: OS firewall rules deny outbound except 127.0.0.1:proxy for agent/tool processes.
- Result: bypass attempts are blocked by the kernel.

## DNS Guardrail
- Run a local resolver and force agent/tool DNS through it; block direct UDP/53 and DoH/DoT from tools.
- Allowlist at the name layer (works even if IPs change), pin short TTLs, and record lookups in the egress ledger.
- For simplicity, disable HTTP/3/QUIC in headless scrapers so web goes via the proxy path.

## Filesystem “Firewall”
- Run dangerous tools in a sandbox with only project:// mounts.
- No write outside project temp/outputs without an explicit lease.
- Pair with a redaction step before any egress of internal/secret data.

## Sensors as Leased Capabilities
- Default‑deny mic/cam; access only via a sidecar capture process we control.
- Leases are timed and project‑scoped; show live in UI; audit events.

## Cluster‑Safe by Construction
- Every worker node runs the same egress proxy and DNS guard; policies pushed from Home Node and enforced locally.
- mTLS between nodes; offloads carry project/policy context; Workers cannot widen scope.

## Human‑Visible Controls
- Network posture per project: Off, Public only, Allowlist, Custom.
- Pre‑offload dialog: diff of payload, domain, cost; one‑click approve/deny with TTL.
- Egress ledger tab: filters (episode/domain/node) + one‑click revoke/kill.

## What You Don’t Need (And Why)
- No deep‑packet inspection or TLS MITM: overkill, brittle, high maintenance. SNI/Host allowlisting + DNS control delivers most benefits.
- No heavyweight service mesh: a per‑node gateway suffices for our single Home Node + invited Workers pattern.

## Known Gaps (Handling)
- UDP/QUIC: prefer HTTP/1.1+2; disable HTTP/3 where possible. Later add CONNECT‑UDP or disallow such tools in “strict” projects.
- Non‑HTTP protocols: permit only if gateway supports (e.g., SSH to specific hosts). Else, sandbox net‑ns with explicit kernel rules.
- Domain fronting/IP‑literal abuse: block IP‑literal CONNECTs by default; require named hosts; cross‑check DNS logs.

## Why This Beats “Trust the Agent”
- Enforceable least‑privilege: risky actions cannot leave scope.
- Measurable and billable: egress ledger links bytes to episodes/budgets.
- Reproducible: snapshots include network leases and egress log; replay explains allow decisions.

## Minimal Rollout Path (Solo‑Friendly)
1) Extend policies with network scopes + leases; surface in UI.
2) Add per‑node egress proxy + DNS guard; route containerized scrapers/browsers first.
3) Wire egress ledger and pre‑offload preview; default posture “Public only”.
4) Expand to all tools; add OS firewall rules for host processes we can’t containerize.
5) Apply the same pattern to Workers; enforce mTLS and policy propagation.

## Planned Configuration (Preview)
These flags are planned to control posture and gateway behavior. Names may evolve before implementation.
- `ARW_NET_POSTURE`: `off|public|allowlist|custom` (per project)
- `ARW_EGRESS_PROXY_ENABLE`: `1` to start per‑node gateway on loopback
- `ARW_EGRESS_PROXY_PORT`: listen port (default 9080)
- `ARW_EGRESS_BLOCK_IP_LITERALS`: `1` to require named hosts
- `ARW_DNS_GUARD_ENABLE`: `1` to enforce local resolver for tools
- `ARW_EGRESS_LEDGER`: path or `state://egress.jsonl`
- `ARW_DISABLE_HTTP3`: `1` for headless scrapers to force H1/H2 via proxy

See also: Guide → Network Posture, Policy, Security Hardening, Clustering.

## Schemas (Preview)
- Policy: Network Scopes & Leases — see `spec/schemas/policy_network_scopes.json`
- Egress Ledger Entry — see `spec/schemas/egress_ledger.json`
