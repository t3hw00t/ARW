# API Reference

Microsummary: HTTP service endpoints for state, debug, and events; stability and semantics. Beta.

- OpenAPI (preview): `../static/openapi.json`.
- Auth: local‑only in default dev; production posture pending; see `docs/guide/security_posture.md`.

Endpoints
- `GET /state/health`: service health, returns `{ status, code }`.
- `GET /debug`: Debug UI when `ARW_DEBUG=1` is set.
- `GET /events`: Server‑Sent Events stream for live updates.
- `GET /state/*`: read‑models (observations, beliefs, world, intents, actions, episodes, self/{agent}).

Semantics
- status vs code: `status` is human‑friendly, `code` is a stable machine hint.
- pagination/filtering: available on selected read‑models (e.g., `/state/models_hashes` supports `limit`, `offset`, `provider`, `sort`, `order`). See endpoint docs.
- stability levels: Stable / Beta / Experimental noted per endpoint as we graduate features.
