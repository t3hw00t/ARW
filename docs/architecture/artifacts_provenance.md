---
title: Artifacts & Provenance
---

# Artifacts & Provenance

Updated: 2025-09-20
Type: Explanation

Artifact entity
- Types: file, message, index, report, snapshot, dataset, model-manifest.
- Fields: id, kind, path/ref, size, checksum, created_at, created_by (agent/profile), tags.

Provenance edges
- `{ inputs[] → tool/model/policy/context → artifact }` captured as events and persisted to a graph store.
- Snapshots bundle effective config (prompts, recipes, policies, versions) for deterministic replay.

Built‑ins (planned)
- Reserved events: `artifact.created`, `artifact.linked`, `artifact.deleted`.
- Endpoint sketch: `POST /artifacts/export` for bundles.

Models (content‑addressed)
- Downloaded models are stored under `state/models/by-hash/<sha256>[.<ext>]`.
- Sidecar manifests `<model-id>.json` include `{ sha256, cas: "sha256", file, name?, path, bytes, provider, verified }`.
- This enables dedupe across projects/nodes and safe verification; consumers should rely on the manifest’s `path`.

Flow (download → CAS → manifest)
```
[Remote URL]
    |
    v
{state}/models/<name>.part  --(resume with If-Range; ETag/Last-Modified in .part.meta)-->  [verify sha256]
    |                                                                                         |
    | ok                                                                                      | mismatch/error
    v                                                                                         v
{state}/models/by-hash/<sha256>[.<ext>]  <------------------------------------------  cleanup + error event
            |
            v
      write manifest
        {state}/models/<id>.json
```

Signals and helpers
- `models.manifest.written` is emitted after writing `<state>/models/<id>.json`.
- Partial downloads keep `<name>.part` plus `<name>.part.meta` (resume validators: `etag`, `last_modified`) for `If-Range` safety.
- Optional preflight (`ARW_DL_PREFLIGHT=1`) performs HEAD to capture `Content-Length` and validators and to enforce size/quota early.
- A hash-based single-flight guard coalesces concurrent download requests for the same artifact and fans out progress/events to all waiting models.
- Throughput EWMA is persisted in `{state_dir}/downloads.metrics.json` and used to admit downloads under hard budgets. Admins can read it (along with live counters) via `GET /admin/state/models_metrics`.
 - Schema: the per‑ID model manifest is defined at `spec/schemas/model_manifest.json`.

GC & quotas
- `POST /admin/models/cas_gc` runs a one‑off sweep of `state/models/by-hash`, deleting unreferenced blobs older than `ttl_days`; emits `models.cas.gc`.
- Optional quota `ARW_MODELS_QUOTA_MB` caps total CAS size; combined with preflight, oversize requests are denied before transfer.

UI
- Show “Evidence” links for risky actions (open the provenance graph slice).

See also: Replay & Time Travel, Versioning & Migration, Data Governance.
