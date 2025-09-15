---
title: Event Topics (Canonical)
---

# Event Topics (Canonical)
Updated: 2025-09-14
Type: Reference

Source of truth for event kinds published by the service. These constants are defined once in code and imported where needed to avoid drift:

- Code constants: `apps/arw-svc/src/ext/topics.rs`

Related docs:
- Explanations → Events Vocabulary: `docs/architecture/events_vocabulary.md`
- Architecture → SSE + JSON Patch Contract: `docs/architecture/sse_patch_contract.md`
- How‑to → Subscribe to Events (SSE): `docs/guide/events_sse.md`
- How‑to → Models Download (HTTP): `docs/guide/models_download.md`

## Topics Table

| Kind                       | Purpose                          | Payload key points |
|----------------------------|----------------------------------|--------------------|
| models.download.progress   | Download lifecycle and errors    | id, status/error, code, budget?, disk?, progress?, downloaded, total? |
| models.changed             | Models list deltas               | op (add/delete/default/downloaded/canceled/error), id, path? |
| models.refreshed           | Default models list refreshed    | count |
| models.manifest.written    | Per‑ID manifest written          | id, manifest_path, sha256 |
| models.cas.gc              | CAS GC sweep summary             | scanned, kept, deleted, deleted_bytes, ttl_days |
| egress.preview             | Pre‑offload destination summary  | id, url (redacted), dest{host,port,protocol}, provider, corr_id |
| egress.ledger.appended     | Egress ledger entry appended     | id?, decision, reason?, dest(host,port,protocol), bytes_in/out, corr_id?, proj?, posture |
| state.read.model.patch     | Read‑model JSON Patch deltas     | id, patch[...] |
| snappy.notice              | Interactive budgets: breach notice | p95_max_ms, budget_ms |
| snappy.detail              | Interactive budgets: periodic detail | p95_by_path{"/path":p95_ms} |
| experiment.activated       | Experiment variant applied       | id, variant, applied{...} |
| rpu.trust.changed          | Trust store changed/reloaded     | count, path |

### Read‑Model Ids (commonly used)

- models — models list + default (patch stream)
- models_metrics — counters + EWMA MB/s (patch stream)
- route_stats — route latencies/hits/errors (patch stream)
- snappy — interactive budgets summary (patch stream)
