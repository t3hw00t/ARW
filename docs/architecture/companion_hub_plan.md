---
title: Companion Hub Roadmap
---

# Companion Hub Roadmap

Updated: 2025-10-26
Type: Explanation

## Purpose

Consolidate every user-facing surface into a role-aware Companion Hub so ARW feels like a single empathetic teammate, while keeping Builder and Ops depth a click away. This roadmap organizes the engineering work needed to:

- Deliver an accessible, long-lived UI shell that scales from laptops to workstation clusters.
- Surface empathy, memory, autonomy, economy, and project signals in one front-stage view.
- Keep policies, guardrails, and ledgers transparent for all users without demanding specialist tooling.

## Guiding Principles

- **Single entry point**: Personas, memory overlays, autonomy lanes, projects, connectors, runtimes, and economy metrics share one navigation model.
- **Role-aware**: Companion (default), Builder (advanced), Ops (deep diagnostics) share data sources but expose controls appropriate to the role.
- **Accessibility-first**: Keyboard parity, high-contrast themes, screen-reader semantics, and low-bandwidth fallbacks are delivery gates.
- **Performance guardrails**: Follow Snappy budgets, lazy-load heavy panels, and reuse SSE read-models to avoid redundant polling.
- **Consent & safety**: Every surface honors leases/capsules; empathy and economy data appears only when policy allows.

## Phased Delivery

### Phase 1 — Companion Hub Foundation

- Reuse launcher Hub scaffolding but reorganize into primary tabs: Today, Personas, Memory, Automations, Economy, Projects, System.
- Build the Daily Brief publisher: hourly/triggered job synthesizes key metrics into natural-language updates delivered through chat/mascot. See How‑to → Daily Brief for the API and client usage.
- Integrate existing read-models (`route_stats`, `context_metrics`, `training_metrics`, `autonomy/lanes`, `projects`, `persona/*`, `models_metrics`, `memory_recent`) and reuse SSE patches for live updates.
- Surface baseline memory quality (lane coverage, freshness, review queue) and autonomy lane/budget summaries while dedicated economy ledgers are still in flight.
- Ship responsive layout with accessibility audits (axe-core + manual screen-reader smoke).

### Phase 2 — Builder & Ops Views

- Builder toggle reveals diff viewers, raw JSON, schema-generated forms, and advanced tuning (logic units, orchestrator runs, connector manifests).
- Ops toggle links directly to `/admin/debug`, Grafana panels, and CLI helpers while keeping the active context visible.
- Serialize view state in URL hashes for deep links and hand-off between roles.

### Phase 3 - Quality & Economy Insights

- Memory quality indicators: contradiction rate, freshness decay, worldview drift, guardrail rejects.
- Economy deck: `/state/economy/*` read-model with jobs, revenue, payouts, compliance posture, and per-project attribution. See How-to → Economy Ledger for endpoint, pagination, and SSE id details.
- Approval inbox: collate guardrail pauses, persona proposals, economy approvals, and connector consent renewals.
- Lightweight offline dashboard: HTML/TUI mini-hub that mirrors top-level summaries for universal access deployments.

## Dependencies & Interfaces

- Launcher (`apps/arw-launcher/src-tauri/ui/hub.js`, sidecar, training, projects).
- Server read-models (`/state/*`, `/events`, autonomy, persona, memory, models, projects, upcoming economy endpoints).
- Prometheus metrics for daily brief highlights (Snappy, plan guardrails, persona feedback, memory drift, economy KPIs).
- Policy enforcement (leases, guardrail gateway) to gate advanced panels.

## Success Metrics

- Time-to-signal: persona mood, autonomy state, and memory health visible within two clicks in Companion mode.
- Accessibility: axe-core CI stays green; manual NVDA/VoiceOver smoke recorded per release.
- Performance: I2F ≤ 50 ms and streaming budgets upheld while tabs are active.
- Adoption: ≥80 % of installs keep Companion Hub as default start surface; ops escalations continue to reference Builder/Ops modes instead of legacy windows.

## Risks & Mitigations

- **UI sprawl**: Resist creating new standalone windows; embed advanced content via Builder/Ops toggles and provide deep links only when necessary.
- **Telemetry overload**: Prioritize curated summaries with progressive disclosure; allow users to mute tiles to avoid cognitive overload.
- **Policy drift**: Ensure every new panel requires the same leases as the underlying APIs; add automated tests to guard against regressions.
- **Offline installs**: Use the same read-model contracts for the mini-dashboard to avoid forked logic.

## Next Steps

1. Implement `t-20251027-companion-hub-foundation` to stand up the reorganized navigation and Daily Brief.
2. Extend read-model coverage for memory quality and economy ledgers (`t-20251027-memory-quality-signals`, `t-20251027-economy-ledger-ui`).
3. Deliver offline/TUI summaries with the Universal Access Kit (`t-20251027-mini-dashboard`).
4. Publish the autonomous-economy handbook (`t-20251027-economy-handbook`) so the Companion Hub economy deck ships with proper guidance.

Keep this roadmap synced with docs/ROADMAP.md, docs/INTERFACE_ROADMAP.md, and the Backlog as phases land.
