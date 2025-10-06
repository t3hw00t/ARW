---
title: Memory & World Schemas
---

# Memory & World Schemas

Updated: 2025-10-06
Type: Reference

Status: Beta

Schemas
- Memory Quarantine Entry — [spec/schemas/memory_quarantine_entry.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/memory_quarantine_entry.json)
- World Diff Review Item — [spec/schemas/world_diff_review_item.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/world_diff_review_item.json)
- Secrets Redaction Rule — [spec/schemas/secrets_redaction_rule.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/secrets_redaction_rule.json)
- Archive Unpack Policy — [spec/schemas/archive_unpack_policy.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/archive_unpack_policy.json)
- DNS Anomaly Event — [spec/schemas/dns_anomaly_event.json](https://github.com/t3hw00t/ARW/blob/main/spec/schemas/dns_anomaly_event.json)

Notes
- Quarantine now lives on the unified server: `GET /admin/memory/quarantine`, `POST /admin/memory/quarantine`, and `POST /admin/memory/quarantine/admit` (admin token required) with SSE events `memory.quarantined` and `memory.admitted`.
- Quarantine entries capture the originating `episode_id` (when available), the `corr_id` used for event stitching, and a normalized `source` slug (`tool`, `ingest`, `world_diff`, `manual`, or adapter-provided value). These fields make it easier to trace how risky memory was produced.
- Review data is persisted directly on each entry: `review.time`, `review.decision` (`admit`, `reject`, or `extract_again`), optional `review.by`, and an operator `review.note`. Admission updates no longer drop the original queue payload.
- Risk markers are deduplicated strings such as `html`, `form`, or extractor-specific tags. Populate them in tooling so dashboards can cluster similar high-risk inputs.
- World diff review integrates in the collaboration flow via `GET /admin/world_diffs`, `POST /admin/world_diffs/queue`, and `POST /admin/world_diffs/decision`; queued/applied/rejected decisions emit `world.diff.*` events.
- Redaction rules are applied to logs, snapshots, and egress previews; keep matchers simple and audited.
- Archive policy is enforced by safe unpackers (path canonicalization + limits), with events `archive.unpacked` and `archive.blocked`.
- DNS anomaly events are best‑effort alerts from the DNS guard; use to surface brief UI banners and suggest posture tightening.

### Memory Quarantine Payload

```json
{
  "id": "q-20251006-152212",
  "project_id": "proj-A",
  "episode_id": "ep-4821",
  "corr_id": "corr-xyz",
  "time": "2025-10-06T13:22:12.345Z",
  "source": "world_diff",
  "content_type": "text/html",
  "content_preview": "<script>alert(1)</script>...",
  "provenance": "https://example.test/page",
  "risk_markers": ["html"],
  "evidence_score": 0.6,
  "extractor": "dom@1",
  "state": "queued",
  "review": null
}
```

- `corr_id` is generated when missing so downstream consumers can continue correlating SSE traffic.
- `evidence_score` is clamped to `[-1, 1]`; values above `0.5` typically indicate high confidence that the input needs review.
- `content_preview` is truncated to 2048 characters; longer payloads should be persisted elsewhere (for example, object storage) and referenced via `provenance`.
- Once an entry is admitted/rejected, the `state` field transitions and `review` becomes an object with reviewer metadata. No fields are removed during admission, making audit trails stable.
- `POST /admin/memory/quarantine/admit` returns both the number of removed rows and the full entry payload after the decision is applied, enabling CLI/UI tooling to display the updated metadata without issuing a second fetch.
- `arw-cli admin review quarantine list|show|admit` wraps these endpoints for operators: `list` supports filters plus `--json/--ndjson/--csv`, `show` fetches a single entry, and `admit --id <id> --id <id2>` can resolve multiple items in one pass (with aggregated JSON output when requested).
