---
title: Persona Telemetry
---

# Persona Telemetry

Updated: 2025-10-19
Type: How-to

Persona telemetry is opt-in. This guide explains how to enable vibe feedback loops, satisfy policy requirements, and inspect the live metrics exposed by `arw-server`.

## Enable Vibe Feedback
- Set telemetry preferences when defining a persona (proposal diff or seed):
  ```json
  {
    "preferences": {
      "telemetry": {
        "vibe": {
          "enabled": true,
          "scope": "workspace"
        }
      }
    }
  }
  ```
- Consent lives inside the persona record; removing the `telemetry.vibe.enabled` flag disables ingestion instantly (`POST /persona/{id}/feedback` returns `412 Precondition Required`).
- Scopes are arbitrary strings (default: owner kind). Every scope requires matching policy or leases before signals are accepted.
- Prefer an admin toggle? Call `POST /admin/persona/{id}/telemetry` with `{ "enabled": true, "scope": "workspace" }` (or `false` to disable) and a `persona:manage` lease.

## Policy & Leases
- Policy evaluation happens first. Grant permission either by:
  - Allowing `telemetry.persona.{scope}` in policy **with** `allow_all` or lease/Cedar rules, or
  - Falling back to a lease capability check.
- Without policy coverage, the server looks for a lease capability `telemetry:persona:{scope}`:
  ```bash
  arw-cli admin lease grant telemetry:persona:workspace --ttl 1h
  ```
- Missing policy and lease coverage produces `403 Persona Telemetry Forbidden`.

## Submit Feedback
- Endpoint: `POST /persona/{id}/feedback`
- Required header: admin authentication (same as persona proposals).
- Body fields:
  | Field | Type | Notes |
  | --- | --- | --- |
  | `kind` | string | Optional label per sample; falls back to the `kind` query param. |
  | `signal` | string | Optional label (`warmer`, `cooler`, etc.). Defaults to `unspecified`. |
  | `strength` | float | Optional range 0.0-1.0 for intensity. |
  | `note` | string | Free-form operator note. |
  | `metadata` | object | Additional structured metadata (sanitized server-side). |
- Optional query `kind=vibe` tags the signal; omit to use the server default.
- Payload can be a single object or an array of objects. The response echoes `{ "status": "accepted", "count": <samples> }`.

## Browse History
- Endpoint: `GET /state/persona/{id}/vibe_history`
- Query: `limit` (default 50) returns the newest samples with timestamp, signal, strength, note, and metadata.
- History lives in SQLite (`persona_vibe_samples`) and retains the latest 50 entries per persona by default.
- Responses include `retain_max` so launchers can display the configured ceiling (clamped 1–500) alongside the history payload.
- Adjust retention with `ARW_PERSONA_VIBE_HISTORY_RETAIN=<limit>` when you need more (or fewer) samples per persona.

## Inspect Metrics
- Endpoint: `GET /state/persona/{id}/vibe_metrics`
- Returns when consent + policy/lease checks succeed:
  ```json
  {
    "persona_id": "persona-1",
    "total_feedback": 4,
    "signal_counts": {
      "warmer": 3,
      "cooler": 1
    },
    "average_strength": 0.65,
    "last_signal": "cooler",
    "last_strength": 0.3,
    "last_updated": "2025-10-18T02:40:10.123Z"
  }
  ```
- Metrics are in-memory only (no disk persistence). Restarting the server clears the counters.
- Responses also include `retain_max` so UIs can mirror the current vibe-history cap alongside metrics.
- Prometheus export adds global/persona totals:
  - `arw_persona_feedback_global_total`
  - `arw_persona_feedback_total{persona="..."}`
  - `arw_persona_feedback_signal_total{persona="...",signal="..."}`

## Access Patterns
- SSE: subscribe to `persona.feedback` on the bus for real-time updates (launchers and dashboards should pair the stream with the metrics snapshot).
- Dashboards: combine `/state/persona`, `/state/persona/{id}/history`, `/state/persona/{id}/vibe_history`, and vibe metrics to surface alignment trends.

## Troubleshooting
- `412 Telemetry Disabled`: persona preferences lack the opt-in flag. Patch the persona or apply a new proposal.
- `403 Persona Telemetry Forbidden`: missing policy entry or lease for the requested scope.
- Metrics missing: ensure the server handling feedback is the same node reading metrics (store is node-local).

