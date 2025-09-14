---
title: Memory & World Schemas
---

# Memory & World Schemas (Planned)

Updated: 2025-09-12

Schemas
- Memory Quarantine Entry — spec/schemas/memory_quarantine_entry.json
- World Diff Review Item — spec/schemas/world_diff_review_item.json
- Secrets Redaction Rule — spec/schemas/secrets_redaction_rule.json
- Archive Unpack Policy — spec/schemas/archive_unpack_policy.json
- DNS Anomaly Event — spec/schemas/dns_anomaly_event.json

Notes
- Quarantine forms the basis of a small review queue: `/state/memory/quarantine` (planned) with SSE events `memory.quarantined` and `memory.admitted`.
- World diff review integrates in the collaboration flow; queued/conflict/applied states reflect in `WorldDiff.*` planned events.
- Redaction rules are applied to logs, snapshots, and egress previews; keep matchers simple and audited.
- Archive policy is enforced by safe unpackers (path canonicalization + limits), with events `Archive.Unpacked` and `Archive.Blocked`.
- DNS anomaly events are best‑effort alerts from the DNS guard; use to surface brief UI banners and suggest posture tightening.
