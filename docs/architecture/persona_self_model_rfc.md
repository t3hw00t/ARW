---
title: Persona & Self-Model RFC (Draft)
---

# Persona & Self-Model RFC (Draft)
Updated: 2025-10-19
Status: Draft
Type: Proposal

## Summary
Define the first-class persona system that lets users cultivate empathetic, transparent agent “characters” while preserving policy safety. The proposal introduces persona read-models (`/state/persona/*`), diff/lease workflows for persona updates, telemetry hooks for vibe feedback, and guardrails that connect the persona to the memory overlay without leaking private signals.

## Goals
- Represent personas as structured, auditable state scoped per workspace/project.
- Let users and trusted automations propose persona changes via diff bundles with explicit approval requirements.
- Bridge episodic/semantic memories and self-model metrics so personas evolve based on real interactions, not opaque magic.
- Capture opt-in empathy signals (tone, pacing, sentiment, confirmations) with consent, accessibility parity, and policy enforcement.
- Surface persona state in Launcher/CLI (persona cards, vibe tuning) with stable APIs for future packs.

## Non-Goals
- Live fine-tuning or on-device embedding updates (future work).
- Cross-user persona sharing/marketplace (tracked under Community Persona Bundles).
- Emotion inference from biosignals (explicitly future/opt-in pending ethics review).

## Architecture Overview

```
+---------------------------+        +------------------------------+
| Persona API (`/actions`)  |------->| Persona Service (Rust)       |
+---------------------------+        |  - Schema validation         |
                                     |  - Policy lease enforcement  |
                                     |  - Diff approval workflow    |
                                     +--------------+---------------+
                                                    |
                                                    v
                                     +------------------------------+
                                     | Persona Store (SQLite)       |
                                     |  persona_entries             |
                                     |  persona_proposals           |
                                     |  persona_history             |
                                     +--------------+---------------+
                                                    |
                                                    v
                         +------------------------------------------+
                         | Read-Models (`/state/persona/*`)         |
                         |  - persona.summary                        |
                         |  - persona.traits                         |
                         |  - persona.vibe_metrics                   |
                         |  - persona.history (paged)                |
                         +------------------+------------------------+
                                            |
                         +------------------v------------------------+
                         | Memory Overlay & Self-Model Bridge        |
                         |  - Persona derives from MAL lanes         |
                         |  - Self-model metrics feed calibrations   |
                         +------------------+------------------------+
                                            |
                         +------------------v------------------------+
                         | UI Integrations (Launcher/CLI/Docs)       |
                         |  - Persona cards                          |
                         |  - Vibe feedback controls                 |
                         |  - Journaling & reflection prompts        |
                         +-------------------------------------------+
```

## Data Model

### `persona_entries`
| Field | Type | Notes |
| --- | --- | --- |
| `id` | `uuid` | Stable persona identifier (per workspace or per project). |
| `owner_kind` | `enum('workspace','project','agent')` | Scope for policy enforcement. |
| `owner_ref` | `text` | Identifier of the scoped owner (workspace id, project id, agent id). |
| `name` | `text` | Display name. |
| `archetype` | `text` | Optional descriptor (e.g., researcher, coach). |
| `traits` | `jsonb` | Keyed object describing worldview, tone, interaction style. |
| `preferences` | `jsonb` | Allowed/avoided behaviors (e.g., cite sources, brevity). |
| `worldview` | `jsonb` | Structured beliefs derived from memory overlay. |
| `vibe_profile` | `jsonb` | Telemetry summaries (tone balance, pacing, sentiment). |
| `calibration` | `jsonb` | Confidence metrics, drift scores. |
| `updated_at` | `unix_ms` | Last applied update. |
| `version` | `integer` | Incremented per applied diff. |

### `persona_proposals`
Stores pending diffs requiring approval.

| Field | Type | Notes |
| --- | --- | --- |
| `proposal_id` | `uuid` | Primary key exposed via API. |
| `persona_id` | `uuid` | Target persona. |
| `submitted_by` | `actor_ref` | User/automation identity. |
| `diff` | `jsonb` | RFC-6902 patch over `persona_entries`. |
| `rationale` | `text` | Human-readable explanation. |
| `telemetry_scope` | `jsonb` | Additional telemetry opt-ins requested. |
| `leases_required` | `jsonb` | Policy scopes enforced before apply. |
| `status` | `enum('pending','approved','rejected','expired')` | Workflow state. |
| `created_at/updated_at` | `unix_ms` | Audit. |

### `persona_history`
Chronological log of applied changes for reproducibility.

## API Surface (Draft)

- `GET /state/persona` → list personas with summary fields.
- `GET /state/persona/{id}` → full persona record, vibe metrics, and links to history.
- `GET /state/persona/{id}/history` → paginated history entries (implemented behind `ARW_PERSONA_ENABLE`).
- `POST /admin/persona/{id}/proposals` → submit diff bundle for review (current API).
- `POST /admin/persona/proposals/{id}/approve|reject` → finalize proposal; approval applies diff and records history.
- `POST /persona/{id}/feedback` → accept vibe feedback signals (publishes `persona.feedback` events).
- Planned follow-up: migrate admin endpoints to `POST /actions (persona.propose|persona.approve)` once lease plumbing lands.

All persona actions emit bus events:
- `persona.proposal.submitted`
- `persona.proposal.approved`
- `persona.updated`
- `persona.vibe.feedback`

## Telemetry & Consent
- Telemetry defaults to `off`. Enabling requires:
  - Explicit user opt-in stored in persona preferences.
  - Policy lease `telemetry:persona:{scope}`.
  - Accessibility check pass (vibe UI must surface on-screen alternatives).
- Collected metrics:
  - `sentiment_avg`, `sentiment_volatility`
  - `response_tempo` (per 1k tokens)
  - `confirmation_rate` (explicit acknowledgments)
  - `misalignment_flags` (when persona deviates from preferences)
- Telemetry aggregation runs through the persona service and publishes sanitized metrics; raw samples are ephemeral (in-memory, no disk persistence) unless retention is explicitly requested via policy.
- `GET /state/persona/{id}/vibe_metrics` exposes the aggregated counters (totals, per-signal counts, averages, last updated) when consent and leases are satisfied; responses return `412` when telemetry is disabled for the persona.
- `GET /state/persona/{id}/vibe_history` returns the latest feedback samples (default 50) for dashboards.
- Admins may `POST /admin/persona/{id}/telemetry` when they hold `persona:manage` to flip consent or update scope defaults without crafting proposals.

## Memory & Self-Model Bridge
- Memory overlay tags episodic/semantic entries with persona relevancy scores.
- Persona service consumes memory events to update worldview facets (e.g., beliefs, preferences).
- Self-model calibration fields (confidence, competence, cost) are mirrored into persona `calibration` so UI can show “how sure” the persona is.
- Guardrails:
  - Only memory items with lease `share:persona` can influence personas.
  - Summaries must cite memory IDs; rehydrate available via existing `/actions (memory.rehydrate)` calls.

## Approval Workflow
1. Proposal submitted via `persona.propose`.
2. Policy layer checks `persona.manage` via policy/leases (`persona:manage` capability required when policy denies).
3. Approver reviews diff (rendered in UI).
4. On approval:
   - Diff applied transactionally.
   - History entry recorded.
   - Event emitted.
   - Optional follow-up tasks triggered (e.g., update vibe UI).
5. On rejection: proposal archived with rationale.
6. Expired proposals auto-reject after configurable TTL.

## Security Considerations
- Persona data stored in the same encrypted state directory as other sensitive read-models.
- Diff apply path enforces maximum size and schema validation to avoid arbitrary JSON injection.
- Telemetry ingestion clamps sample rates and enforces max retention (default 7 days).
- All persona events include anonymized actor references for auditing.
- CLI/Launcher must surface consent notices before enabling telemetry or persona sharing.

## UI Hooks
- Persona card component (Launcher sidecar):
  - Displays top traits, tone sliders, worldview summary, recent reflections.
  - Provides “Propose change” button (opens diff editor).
- Vibe feedback widget:
  - Quick reactions (warmer/cooler, faster/slower, more/less formal).
  - Sends `persona.feedback` actions with reason codes.
- Journaling prompts:
  - Optional daily/weekly prompts surfaced via notifications.
  - Stored as memory entries tagged `persona_reflection`.

## CLI Hooks
- `arw-cli persona list`
- `arw-cli persona show <id>`
- `arw-cli persona propose --file diff.json`
- `arw-cli persona approve <proposal_id>`

## Migration Plan
1. **Schema prep** — add tables and read-model scaffolding guarded by feature flag `ARW_PERSONA_ENABLE`.
2. **API scaffolding** — land read endpoints, stub actions returning “feature disabled” until flag on.
3. **Telemetry opt-ins** — implement telemetry lease scopes and CLI toggles.
4. **Launcher integration** — deliver persona card and vibe feedback UI.
5. **General availability** — enable flag by default after empathy research sprint validates UX.

## Open Questions
- Should personas be scoped per project by default or allow multiple per project?
- How do we reconcile conflicting feedback from multiple collaborators?
- What anonymization is required if personas are shared externally?
- How do we surface persona drift alerts without overwhelming users?
- Do we need per-persona resource budgets (token/latency) or reuse self-model budgets?

## Next Actions
- Review this RFC with Kernel/Research/Collaboration owners.
- Define acceptance tests for schema migrations and diff workflow.
- Coordinate with privacy counsel on telemetry consent copy.
- Prototype persona card UI using fake data to validate ergonomics.
