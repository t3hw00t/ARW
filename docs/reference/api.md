# API Reference

Updated: 2025-09-22
Type: Reference

Microsummary: Public endpoints, admin surfaces, specs, and eventing. Stable/experimental flags are surfaced in specs; deprecations emit standard headers.

- Default base URL: `http://127.0.0.1:8091` (unified `arw-server`; override with `ARW_PORT`).
- Specs in repo: [spec/openapi.yaml](https://github.com/t3hw00t/ARW/blob/main/spec/openapi.yaml), [spec/asyncapi.yaml](https://github.com/t3hw00t/ARW/blob/main/spec/asyncapi.yaml), [spec/mcp-tools.json](https://github.com/t3hw00t/ARW/blob/main/spec/mcp-tools.json)
- Specs at runtime: `GET /spec/openapi.yaml` (generated from code annotations), `GET /spec/asyncapi.yaml`, `GET /spec/mcp-tools.json`, `GET /spec/schemas/{file}`, `GET /spec/index.json`
  - Alias: `GET /spec/openapi.gen.yaml` (same as `/spec/openapi.yaml`)
- Catalog: `GET /catalog/index` (YAML) and `GET /catalog/health` (JSON)
- `/state/*`: read-models for actions, contributions, episodes, leases, egress, policy, models, and self snapshots.
- Auth: Local-only by default; for admin endpoints set `ARW_ADMIN_TOKEN` and send `Authorization: Bearer <token>` or `X-ARW-Admin`.

## Endpoint overview

### Core triad (served by `arw-server` on 8091)

| Method | Path | Purpose | Stability |
| --- | --- | --- | --- |
| GET | `/healthz` | Service readiness probe. | stable |
| GET | `/about` | Metadata including endpoints index and counts. | stable |
| POST | `/actions` | Submit an action to the triad queue; returns `{ id }`. | beta |
| GET | `/actions/{id}` | Fetch action status by identifier. | beta |
| POST | `/actions/{id}/state` | Worker lifecycle update for a submitted action. | beta |
| GET | `/events` | SSE stream with optional `?replay=`/`?after=` and `Last-Event-ID`. | stable |
| GET | `/state/actions` | Recent actions (supports `?limit=`). | beta |
| GET | `/state/experiments` | Recent experiment events snapshot. | beta |
| GET | `/state/contributions` | Kernel contributions list (latest 200). | beta |
| GET | `/state/episodes` | Episode rollups grouped by `corr_id`. | beta |
| GET | `/state/route_stats` | Bus throughput plus per-route counters. | beta |
| POST | `/leases` | Allocate a capability lease; body supplies `capability`, `scope?`, `ttl_secs?`, `budget?`. | experimental |
| GET | `/state/leases` | Snapshot of active leases. | experimental |
| GET | `/state/egress` | Recent egress ledger rows (supports `?limit=`). | beta |
| GET | `/state/egress/settings` | Effective egress posture and toggles. | beta |
| POST | `/egress/settings` | Persist posture/toggle updates (admin token required). | beta |
| POST | `/egress/preview` | Dry-run egress decision for a URL/method. | beta |
| GET | `/state/policy` | Current ABAC policy snapshot. | experimental |
| POST | `/policy/reload` | Reload policy from disk/env (admin token required). | experimental |
| POST | `/policy/simulate` | Evaluate a candidate ABAC request payload. | experimental |
| GET | `/state/models` | Model catalog read-model (`{"items": [...]}`). | beta |

### Specs and catalog (served by `arw-server` on 8091)

| Method | Path | Purpose | Stability |
| --- | --- | --- | --- |
| GET | `/spec/openapi.yaml` | OpenAPI document for the unified server. | stable |
| GET | `/spec/asyncapi.yaml` | AsyncAPI schema for event streams. | stable |
| GET | `/spec/mcp-tools.json` | MCP tools manifest. | stable |
| GET | `/spec/health` | Presence/size summary for published spec artifacts. | stable |
| GET | `/spec/schemas/{file}` | JSON Schemas referenced by the API. | stable |
| GET | `/spec/index.json` | Index of published specs. | stable |
| GET | `/catalog/index` | Interface catalog (YAML). | stable |
| GET | `/catalog/health` | Catalog health probe. | stable |

All endpoints above default to `http://127.0.0.1:8091` unless an alternate bind/port is configured.

Actions (unified server)
- `POST /actions` — submit action; returns `{ id }` (202)
- `GET /actions/{id}` — fetch action state
- `POST /actions/{id}/state` — worker lifecycle update

Memory
- `POST /admin/memory/apply` — insert or update a memory item (admin helper)
- `GET /admin/memory` — list recent memory items (admin helper; supports `lane`/`limit`)
- `GET /state/memory/recent` — most recent memories (per lane)
- Action-first interface: `POST /actions (memory.upsert|memory.search|memory.pack)` handles durable updates, retrieval, and packing with event telemetry.
- Review queue (admin): `GET /admin/memory/quarantine`, `POST /admin/memory/quarantine`, `POST /admin/memory/quarantine/admit` — track quarantined extracts before admitting to world/memory lanes.
- World diff decisions (admin): `GET /admin/world_diffs`, `POST /admin/world_diffs/queue`, `POST /admin/world_diffs/decision` — queue diffs, record human decisions, and emit `world.diff.*` events.

Connectors
- `GET /state/connectors` — list registered connector manifests (secrets elided)
- `POST /connectors/register` — register a manifest (admin-gated)
- `POST /connectors/token` — set/update token/refresh token (admin-gated)

Logic Units & Config
- `GET /logic-units` — catalog installed logic units
- `GET /state/logic_units` — read-model snapshot
- `POST /logic-units/install` — install a logic unit (admin-gated)
- `POST /logic-units/apply` — apply a patch set with optional schema validation (admin-gated)
- `POST /logic-units/revert` — revert to a config snapshot (admin-gated)
- `GET /state/config` — effective config JSON
- `POST /patch/apply` — apply patches (admin-gated)
- `POST /patch/revert` — revert to snapshot (admin-gated)
- `GET /state/config/snapshots` — list config snapshots
- `GET /state/config/snapshots/{id}` — get a specific snapshot
- `POST /patch/validate` — validate a config against a JSON Schema
- `GET /state/schema_map` — current schema mapping used for inference
- `POST /patch/infer_schema` — map a target path to schema/pointer

Goldens & Experiments (admin token required)
- `GET /admin/goldens/list` — list stored goldens (`?proj=default`)
- `POST /admin/goldens/add` — append a golden `{ proj, kind, input, expect }`
- `POST /admin/goldens/run` — evaluate goldens and emit `goldens.evaluated`
- `POST /admin/experiments/define` — register experiment variants and knobs
- `POST /admin/experiments/start` — emit `experiment.started` with assignment/budget hints
- `POST /admin/experiments/stop` — emit `experiment.completed`
- `POST /admin/experiments/assign` — emit `experiment.variant.chosen`
- `POST /admin/experiments/run` — run A/B/n on goldens and emit `experiment.result`
- `POST /admin/experiments/activate` — apply winner hints (emits `experiment.activated`)
- `GET /admin/experiments/list` — list experiment definitions
- `GET /admin/experiments/scoreboard` — last-run metrics per variant
- `GET /admin/experiments/winners` — persisted winners snapshot

Tool Forge & Guardrails (admin token required)
- `GET /admin/tools` — enumerate registered tools with metadata from `arw_core::introspect_tools()`.
- `POST /admin/tools/run` — invoke a tool (e.g., `ui.screenshot.capture`, `guardrails.check`); honors ingress/egress gates.
- `GET /admin/tools/cache_stats` — action cache counters (hit/miss/coalesced/errors/bypass plus capacity/ttl/entries).
- `GET /state/guardrails_metrics` — guardrails circuit-breaker and retry counters for observability.

Semantics
- status vs code: RFC 7807 ProblemDetails for errors; otherwise endpoint-specific JSON.
- pagination/filtering: `GET /state/actions` and `GET /state/egress` support a `limit` query parameter (default 200).
- stability: experimental → beta → stable → deprecated → sunset (see Interface Catalog and Deprecations pages).
- deprecations: deprecated operations advertise `Deprecation: true`; `Sunset: <date>` when scheduled; `Link: rel="deprecation"` points to the doc.
- operationId: snake_case with `_doc` suffix (enforced by Spectral; code-generated OpenAPI is linted in CI).

## Models

`GET /state/models`

- Returns the model catalog read-model as `{ "items": [...] }`, loading `state/models.json` when present and falling back to defaults.
- Items include at minimum `id`, `provider`, and `status`; connectors may extend the shape.
- Triad read-models follow the same pattern for other slices such as `/state/actions`, `/state/contributions`, `/state/egress`, `/state/leases`, `/state/policy`, `/state/self`, and `/state/orchestrator/jobs`.

Example (unified server)

```bash
curl -sS http://127.0.0.1:8091/state/models | jq
```

Sample response (defaults)

```json
{
  "items": [
    {
      "id": "llama-3.1-8b-instruct",
      "provider": "local",
      "status": "available"
    },
    {
      "id": "qwen2.5-coder-7b",
      "provider": "local",
      "status": "available"
    }
  ]
}
```

!!! note "Admin downloads and CAS endpoints"
    `arw-server` exposes model orchestration helpers under `/admin/models/*`:

    - `POST /admin/models/download` — queue a download `{ id, source_url?, resume? }` and stream progress over `models.download.progress` events.
    - `POST /admin/models/download/cancel` — request cancellation for a queued or running download `{ "id": "<model-id>" }`.
    - `POST /admin/models/concurrency` / `GET /admin/models/concurrency` — manage runtime concurrency limits.
    - `GET  /admin/models/jobs` — inspect active jobs and inflight hashes.
    - `POST /admin/models/cas_gc` — trigger garbage collection for stored CAS blobs (returns a summary).
    - `GET  /events` — subscribe for progress/errors (filter with `?prefix=models.` for model activity).
    - `GET  /admin/debug` — debug UI when `ARW_DEBUG=1`.

    Example download flow:

    ```bash
    curl -sS -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
      -H 'content-type: application/json' \
      -d '{"id":"llama-3.1-8b-instruct"}' \
      http://127.0.0.1:8091/admin/models/download | jq

    # Inspect active jobs
    curl -sS -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
      http://127.0.0.1:8091/admin/models/jobs | jq

    # Watch progress
    curl -N -H "Authorization: Bearer $ARW_ADMIN_TOKEN" \
      "http://127.0.0.1:8091/events?prefix=models.download"
    ```

Egress
- `GET /state/egress` — recent egress ledger rows `{ id, time, decision, reason?, dest_host?, dest_port?, protocol?, bytes_in?, bytes_out?, corr_id?, proj?, posture }`
- `GET /state/egress/settings` — effective egress posture and toggles
- `POST /egress/settings` — update toggles and persist to config (admin-gated)
- `POST /egress/preview` — dry-run URL+method against policy, allowlist, and guards `{ allow, reason?, host, port, protocol }`

Example — `GET /state/egress`

```bash
curl -sS "http://127.0.0.1:8091/state/egress?limit=1" | jq
```

```json
{
  "items": [
    {
      "id": 101,
      "time": "2025-09-15T12:34:56.789Z",
      "decision": "allow",
      "reason": "http",
      "dest_host": "api.github.com",
      "dest_port": 443,
      "protocol": "https",
      "bytes_in": 32768,
      "bytes_out": 0,
      "corr_id": "act_123",
      "proj": "proj-demo",
      "posture": "standard"
    }
  ]
}
```

SSE
- `GET /events?prefix=egress.` — stream `egress.ledger.appended` events as they occur
- Envelope payload example:
  ```json
  {
    "time": "2025-09-15T12:00:00Z",
    "kind": "egress.ledger.appended",
    "payload": {
      "id": 42,
      "decision": "allow",
      "dest_host": "api.github.com",
      "dest_port": 443,
      "protocol": "https",
      "bytes_in": 12345,
      "corr_id": "act_123",
      "proj": "proj-demo",
      "posture": "standard"
    }
  }
  ```
- Example client: `curl -N http://127.0.0.1:8091/events?prefix=egress.`
