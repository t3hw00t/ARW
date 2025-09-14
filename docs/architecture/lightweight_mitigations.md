---
title: Lightweight Mitigations
---

# Lightweight Mitigations (Plan)

Updated: 2025-09-12

This plan layers pragmatic, low‑overhead safeguards on top of ARW’s policy/egress/firewall work. They are compatible with snapshots/replay, staging approvals, and the project‑scoped design.

## Memory Write Policy & Quarantine
- Treat retrieved text as untrusted; only beliefs with explicit provenance and a positive evidence score can enter long‑term memory.
- Quarantine risky items (HTML/scripts/forms/JS‑heavy pages); require a safer extractor or explicit review before admitting.
- Emit `memory.quarantined` and `memory.admitted` events; surface a small review queue in the sidecar.

## Project Isolation by Construction
- Strict per‑project namespaces for caches, embeddings, semantic indexes; no cross‑project mounts by default.
- “Export views” not “shared stores”: imported views are read‑only and revocable.

## Belief‑Graph Ingest Rules
- World diffs from collaborators enter a review queue; conflicts/contradictions are visible; resolve or park.
- Emit `world.diff.queued`, `world.diff.conflict`, `world.diff.applied` events.

## Cluster Attestation & Manifest Pinning
- Nodes publish signed manifests (tool versions, model hashes, sandbox profiles).
- Scheduler targets only nodes whose manifest matches the workspace spec.
- Emit `cluster.manifest.published`, `cluster.manifest.trusted`, `cluster.manifest.rejected`.

## Secrets Hygiene
- Secrets live only in a project vault; never echoed into prompts/logs.
- Redaction pass on snapshots and egress previews; automatic secret scanner on artifacts.
- Emit `secrets.redacted` and `secrets.found` (severity=warn) with minimal context.

## Hardened Headless Browsing
- Disable service workers and HTTP/3; same‑origin fetches only unless allow‑listed.
- DOM‑to‑text extractors drop scripts/styles; all network via the egress proxy.

## Safe Archive Handling
- Decompress to a temp jail; canonicalize paths; enforce size/time limits; block nested archives beyond small depth.
- Emit `archive.unpacked` with counts/bytes and `archive.blocked` with reason.

## DNS Guard + Anomaly Detection
- All agent DNS via local resolver; deny raw UDP/53 and DoH/DoT from tools; rate‑limit lookups.
- Alert on high‑entropy domain bursts (`dns.anomaly`).

## Accelerator Hygiene
- Zero VRAM/workspace buffers between jobs; disable persistence mode where possible; prefer per‑job processes over long‑lived shared contexts.

## Co‑Drive Role Separation
- Roles: view/suggest/drive. “Drive” cannot widen permissions or approve leases; risky actions still hit the Staging Area.
- Tag remote actions in the timeline; show reviewer/driver identity.

## Event Integrity (Cluster)
- mTLS; per‑episode nonces; monotonically increasing sequence numbers; idempotent tool actions with dedupe keys.
- Reject out‑of‑order/duplicate control events; log as `cluster.event.rejected` with reason.

## Context Rehydration Guard
- Redaction + classification check before retrieved chunks enter prompts that might go remote later.
- Show a “potentially exportable” badge and require an egress lease if offload happens.

## Operational Guardrails (Solo‑Friendly)
- One “security posture” per project: Relaxed / Standard / Strict (Strict disables remote JS, blocks non‑HTTP protocols, requires manual review for world diffs).
- Egress ledger retention + daily review; one‑click revoke/blacklist for suspicious hosts.
- Quarterly key rotation + manifest re‑sign; monthly dependency sweep with golden tests and snapshot diffs.
- Seeded red‑team tests in CI: prompt‑injection sample, zip‑slip sample, SSRF sample, secrets‑in‑logs detector.

See also: Egress Firewall; Network Posture; Threat Model; Security Hardening; Cluster Federation.
