---
title: Memory & World Schemas
---

# Memory & World Schemas

Updated: 2025-09-16
Type: Reference

Status: Beta

Schemas
- Memory Quarantine Entry — [spec/schemas/memory_quarantine_entry.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/memory_quarantine_entry.json)
- World Diff Review Item — [spec/schemas/world_diff_review_item.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/world_diff_review_item.json)
- Secrets Redaction Rule — [spec/schemas/secrets_redaction_rule.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/secrets_redaction_rule.json)
- Archive Unpack Policy — [spec/schemas/archive_unpack_policy.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/archive_unpack_policy.json)
- DNS Anomaly Event — [spec/schemas/dns_anomaly_event.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/dns_anomaly_event.json)

Notes
- Quarantine now lives on the unified server: `GET /admin/state/memory/quarantine`, `POST /admin/memory/quarantine`, and `POST /admin/memory/quarantine/admit` (admin token required) with SSE events `memory.quarantined` and `memory.admitted`.
- World diff review integrates in the collaboration flow via `GET /admin/state/world_diffs`, `POST /admin/world_diffs/queue`, and `POST /admin/world_diffs/decision`; queued/applied/rejected decisions emit `world.diff.*` events.
- Redaction rules are applied to logs, snapshots, and egress previews; keep matchers simple and audited.
- Archive policy is enforced by safe unpackers (path canonicalization + limits), with events `archive.unpacked` and `archive.blocked`.
- DNS anomaly events are best‑effort alerts from the DNS guard; use to surface brief UI banners and suggest posture tightening.
