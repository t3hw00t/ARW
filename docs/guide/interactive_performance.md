# Interactive Performance Configuration
Updated: 2025-09-15
Type: How‑to

Defaults (can be kept in-repo): `configs/snappy.yaml`.

Environment overrides:

- `ARW_SNAPPY_I2F_P95_MS`: p95 interaction-to-first-feedback target (default `50`)
- `ARW_SNAPPY_FIRST_PARTIAL_P95_MS`: p95 first useful partial target (default `150`)
- `ARW_SNAPPY_CADENCE_MS`: steady stream cadence budget (default `250`)
- `ARW_SNAPPY_FULL_RESULT_P95_MS`: p95 full result target (default `2000`)
- `ARW_SNAPPY_PROTECTED_ENDPOINTS`: CSV prefixes treated as interactive (default `/debug,/state/,/chat/,/admin/events`)
- `ARW_SNAPPY_PUBLISH_MS`: interactive read‑model publish interval ms (default `2000`)
- `ARW_SNAPPY_DETAIL_EVERY`: seconds between detailed p95 breakdown events (optional)

SSE resume and deltas: see `architecture/sse_patch_contract.md`.

Topics
- Canonical constants: `crates/arw-topics/src/lib.rs`.
- Read‑model patches on `state.read.model.patch` with id=`snappy`.
- Notices on breach: `snappy.notice` with `{ p95_max_ms, budget_ms }`.
- Optional details (if `ARW_SNAPPY_DETAIL_EVERY>0`): `snappy.detail` with `{ p95_by_path: {"/path": p95_ms} }`.
