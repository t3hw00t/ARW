---
title: Scaling & Sharing (Opt‑In)
---

# Scaling & Sharing (Opt‑In)

Updated: 2025-09-16
Type: How‑to

Agent Hub (ARW) is local‑first and privacy‑first by default. When a project needs more muscle or collaboration, you can opt‑in to scale and share — with clear boundaries and your approval at every step.

## Pool Compute
- Add your own GPU box or a trusted friend’s machine as a worker.
- Heavy jobs offload under your rules, budget, and policies.
- Preview what would be sent (inputs, sizes, estimated cost) before anything leaves your machine.

Related
- Architecture: Federated Clustering — `architecture/cluster_federation.md`
- Guide: Network Posture (egress modes) — `guide/network_posture.md`
- Architecture: Egress Firewall (policy‑backed gateway) — `architecture/egress_firewall.md`

## Live Co‑Drive
- Share an agent session so collaborators can watch, suggest, or take the wheel with your approval.
- Risky actions land in a staging area and wait for an explicit go‑ahead.

Related
- Architecture: Capability & Consent Ledger — `architecture/capability_consent_ledger.md`
- Guide: Permissions & Policies — `guide/policy_permissions.md`

## Clear Boundaries
- You see what would be sent, to whom, and the estimated cost.
- An egress ledger records offloads for review and auditing.

Related
- Reference: Telemetry & Privacy — `reference/telemetry_privacy.md`
- Architecture: Data Governance & Privacy — `architecture/data_governance.md`

## Fair Splits
- Contributions (GPU time, tokens, tasks) are metered per collaborator.
- Enables transparent revenue splits for shared projects later.

Status
- Some components are in active development or planned. See the roadmap and clustering/egress design docs for current status and interfaces. For a live snapshot of known nodes and their advertised capabilities, call `GET /admin/state/cluster` (or watch the `cluster_nodes` read-model).

See also
- Features — `FEATURES.md`
- Roadmap — `ROADMAP.md`
